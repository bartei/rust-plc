//! UPP response decoder.
//!
//! Every cell in manual §7's "Answer" column is one [`Decoder`]
//! variant. The decoder takes the response bytes between the address
//! prefix and the trailing `CR` (the "payload") and produces a typed
//! [`DecodedValue`].
//!
//! The wire is plain ASCII with no checksum; once we've confirmed
//! the address prefix matches the request, decoding is just numeric
//! parsing. Tests in this module reproduce every "Answer" example
//! from the manual.

use crate::error::UppError;

/// Which channel to extract from `ek` (one-channel + ratio in one
/// `SSSSSQQQQQ` answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementChannel {
    /// First 5 digits — one-channel temperature.
    OneChannel,
    /// Last 5 digits — ratio temperature.
    Ratio,
}

/// One of the decoded payload shapes from manual §7.
///
/// Variants carry the most natural Rust type for the value (an
/// `f64` for temperatures and analog quantities, integers for
/// enums and counters, `String` for free-form returns like
/// `na`/`vs`). The runtime FB layer chooses how to pack each into
/// `st_ir::Value` per the device profile.
#[derive(Debug, Clone, PartialEq)]
pub enum DecodedValue {
    /// Temperature in °C or °F (depending on the device's `fh`
    /// setting — the decoder does not know which).
    Temperature(f64),
    /// Pair (one-channel, ratio) temperatures in °C or °F.
    TemperaturePair { one_channel: f64, ratio: f64 },
    /// Internal-temperature reading (3-digit integer; °C or °F).
    InternalTemp(f64),
    /// Unsigned integer payload (signal strength, raw counters).
    UInt(u32),
    /// 0..1.000 emissivity / transmittance (4-digit field /1000).
    Per1000(f64),
    /// Small enum / selector (response time, op mode, baud rate
    /// index, …). Range checking is the caller's job.
    Enum(u8),
    /// Boolean flag (laser, °C/°F bit, …).
    Bool(bool),
    /// 8-hex-digit lo+hi pair (basic range, sub range, limits-of-`em?`).
    HexPair { lo: u16, hi: u16 },
    /// Acknowledgement token — `b"ok"` or `b"no"` per manual §7.
    Ack(bool),
    /// Free-form ASCII (`na`, `vs`, `vc`, `pa`, …). The runtime
    /// layer parses this further when the device profile demands it.
    Text(String),
}

/// Per-command decoder, picked at the device-profile level. Each
/// variant lines up 1:1 with one or more manual §7 rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoder {
    /// `ms` — 5 decimal digits, last is 1/10 °C/°F.
    Temp5dTenth,
    /// `ek` — 10 decimal digits, two adjacent 5-digit values.
    TempPair5dTenth { channel: MeasurementChannel },
    /// `gt` — 3 decimal digits, integer °C or °F.
    Temp3dInt,
    /// `tr` — 4 decimal digits, integer 0000..1500.
    U16Dec4,
    /// `em`, `et`, `ev` — 4 decimal digits / 1000.0
    /// (ε ∈ 0.050..1.000, K ∈ 0.800..1.200, etc.).
    U16DecMilli,
    /// `ez`, `ka`, `la`, `fh`, `as` — 1 decimal digit selector.
    Enum1,
    /// `lz` — 1 decimal digit, but the manual lists 0..9 (more
    /// values than `Enum1`, so callers may want a different range
    /// check).
    Enum1Wide,
    /// `la` answer is exactly the bytes "0" or "1".
    BoolDigit,
    /// `mb`, `me` — 8 hex digits split into two 4-hex-digit halves
    /// (lo, hi).
    HexPair8,
    /// `ok` / `no` ack from a pure-write command.
    Ack,
    /// `na` (16-char device type), `vs`, `vc`, `pa`, `sn`, `bn`,
    /// `gt`-style readbacks where the caller wants the raw ASCII to
    /// hand to a domain-specific parser later.
    Text,
}

impl Decoder {
    /// Decode the payload (already stripped of the leading
    /// address prefix and the trailing `CR`).
    pub fn decode(&self, payload: &[u8]) -> Result<DecodedValue, UppError> {
        match self {
            Decoder::Temp5dTenth => decode_temp_5d(payload).map(DecodedValue::Temperature),
            Decoder::TempPair5dTenth { channel } => {
                let pair = decode_temp_pair_5d(payload)?;
                let DecodedValue::TemperaturePair { one_channel, ratio } = pair else {
                    unreachable!("decode_temp_pair_5d returns the right variant")
                };
                Ok(match channel {
                    MeasurementChannel::OneChannel => DecodedValue::Temperature(one_channel),
                    MeasurementChannel::Ratio => DecodedValue::Temperature(ratio),
                })
            }
            Decoder::Temp3dInt => decode_temp_3d(payload).map(DecodedValue::InternalTemp),
            Decoder::U16Dec4 => decode_u_n_dec(payload, 4).map(|v| DecodedValue::UInt(v as u32)),
            Decoder::U16DecMilli => {
                let v = decode_u_n_dec(payload, 4)?;
                Ok(DecodedValue::Per1000(v as f64 / 1000.0))
            }
            Decoder::Enum1 => decode_u_n_dec(payload, 1).map(|v| DecodedValue::Enum(v as u8)),
            Decoder::Enum1Wide => decode_u_n_dec(payload, 1).map(|v| DecodedValue::Enum(v as u8)),
            Decoder::BoolDigit => decode_bool_digit(payload).map(DecodedValue::Bool),
            Decoder::HexPair8 => decode_hex_pair_8(payload),
            Decoder::Ack => decode_ack(payload).map(DecodedValue::Ack),
            Decoder::Text => Ok(DecodedValue::Text(
                std::str::from_utf8(payload)
                    .map_err(|_| UppError::BadResponse("non-ASCII text payload".into()))?
                    .to_string(),
            )),
        }
    }
}

// ── Decoding helpers ───────────────────────────────────────────────

fn decode_u_n_dec(payload: &[u8], n: usize) -> Result<u64, UppError> {
    if payload.len() != n {
        return Err(UppError::BadResponse(format!(
            "expected {n} decimal digits, got {} bytes",
            payload.len()
        )));
    }
    let s = std::str::from_utf8(payload)
        .map_err(|_| UppError::BadResponse("non-ASCII decimal payload".into()))?;
    s.parse::<u64>()
        .map_err(|e| UppError::BadResponse(format!("decimal parse: {e}")))
}

fn decode_temp_5d(payload: &[u8]) -> Result<f64, UppError> {
    let v = decode_u_n_dec(payload, 5)?;
    Ok(v as f64 / 10.0)
}

fn decode_temp_pair_5d(payload: &[u8]) -> Result<DecodedValue, UppError> {
    if payload.len() != 10 {
        return Err(UppError::BadResponse(format!(
            "ek answer must be 10 digits, got {}",
            payload.len()
        )));
    }
    let one = decode_temp_5d(&payload[0..5])?;
    let two = decode_temp_5d(&payload[5..10])?;
    Ok(DecodedValue::TemperaturePair {
        one_channel: one,
        ratio: two,
    })
}

fn decode_temp_3d(payload: &[u8]) -> Result<f64, UppError> {
    let v = decode_u_n_dec(payload, 3)?;
    // Manual §4.17 / row `gt`: 000..098 °C or 032..210 °F. The
    // pyrometer doesn't report fractional internal temperature.
    Ok(v as f64)
}

fn decode_bool_digit(payload: &[u8]) -> Result<bool, UppError> {
    match payload {
        b"0" => Ok(false),
        b"1" => Ok(true),
        _ => Err(UppError::BadResponse(format!(
            "expected '0' or '1', got {:?}",
            String::from_utf8_lossy(payload)
        ))),
    }
}

fn decode_hex_pair_8(payload: &[u8]) -> Result<DecodedValue, UppError> {
    if payload.len() != 8 {
        return Err(UppError::BadResponse(format!(
            "expected 8 hex digits (XXXXYYYY), got {}",
            payload.len()
        )));
    }
    let s = std::str::from_utf8(payload)
        .map_err(|_| UppError::BadResponse("non-ASCII hex payload".into()))?;
    let lo = u16::from_str_radix(&s[0..4], 16)
        .map_err(|e| UppError::BadResponse(format!("hex lo parse: {e}")))?;
    let hi = u16::from_str_radix(&s[4..8], 16)
        .map_err(|e| UppError::BadResponse(format!("hex hi parse: {e}")))?;
    Ok(DecodedValue::HexPair { lo, hi })
}

fn decode_ack(payload: &[u8]) -> Result<bool, UppError> {
    match payload {
        b"ok" => Ok(true),
        b"no" => Ok(false),
        _ => Err(UppError::BadResponse(format!(
            "expected 'ok' or 'no', got {:?}",
            String::from_utf8_lossy(payload)
        ))),
    }
}

// ── Spec tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual §7 example: "Answer: '0970' + <CR> means Emissivity =
    /// 0.97 or 97.0%". Payload (without CR) is `b"0970"`.
    #[test]
    fn manual_example_emissivity_decode() {
        let v = Decoder::U16DecMilli.decode(b"0970").unwrap();
        match v {
            DecodedValue::Per1000(x) => assert!((x - 0.970).abs() < 1e-9, "got {x}"),
            other => panic!("expected Per1000, got {other:?}"),
        }
    }

    /// Manual §7 example: "Answer: '00em0853' + <CR> changes the
    /// Emissivity to 0.853 or 85.3%". The address prefix is stripped
    /// before reaching the decoder, so we decode `b"0853"`.
    #[test]
    fn manual_example_write_echo_emissivity() {
        let v = Decoder::U16DecMilli.decode(b"0853").unwrap();
        assert_eq!(v, DecodedValue::Per1000(0.853));
    }

    /// Manual §7 example: "00em? answer could be 00501000 + <CR>,
    /// which means ε can vary between 0.050 and 1.000".
    #[test]
    fn manual_example_limits_emissivity() {
        let v = Decoder::HexPair8.decode(b"00501000").unwrap();
        // Manual writes 0050..1000 in DECIMAL but the wire format
        // for limits queries is HEX per the `mb`/`me` rows. The
        // emissivity-limits example happens to be unambiguous (0050
        // and 1000 are the same in dec and hex). We pin the actual
        // returned interpretation: as hex pair this reads 0x0050 lo,
        // 0x1000 hi — which the runtime layer scales by the same
        // /1000 factor as the value itself.
        assert_eq!(v, DecodedValue::HexPair { lo: 0x0050, hi: 0x1000 });
    }

    /// `ms` row — 5 decimal digits with last = 1/10. Example
    /// "12345" → 1234.5 °C.
    #[test]
    fn measuring_value_5d_tenth() {
        let v = Decoder::Temp5dTenth.decode(b"12345").unwrap();
        assert_eq!(v, DecodedValue::Temperature(1234.5));
    }

    /// 100 °C exactly → "01000".
    #[test]
    fn measuring_value_low_end() {
        assert_eq!(
            Decoder::Temp5dTenth.decode(b"01000").unwrap(),
            DecodedValue::Temperature(100.0)
        );
    }

    /// Top of the IGAR 6 range: 2550.0 °C → "25500".
    #[test]
    fn measuring_value_high_end() {
        assert_eq!(
            Decoder::Temp5dTenth.decode(b"25500").unwrap(),
            DecodedValue::Temperature(2550.0)
        );
    }

    /// `ek` 10-digit answer — example one-channel 1234.5 °C, ratio
    /// 1235.0 °C → "1234512350". Both `MeasurementChannel` selectors
    /// pull the right half.
    #[test]
    fn measuring_pair_decodes_both_channels() {
        let one = Decoder::TempPair5dTenth {
            channel: MeasurementChannel::OneChannel,
        }
        .decode(b"1234512350")
        .unwrap();
        let ratio = Decoder::TempPair5dTenth {
            channel: MeasurementChannel::Ratio,
        }
        .decode(b"1234512350")
        .unwrap();
        assert_eq!(one, DecodedValue::Temperature(1234.5));
        assert_eq!(ratio, DecodedValue::Temperature(1235.0));
    }

    /// Internal temperature row `gt` — manual: "DDD 3 decimal digits
    /// 000 to 098 °C or 032 to 210 °F".
    #[test]
    fn internal_temp_3d() {
        assert_eq!(
            Decoder::Temp3dInt.decode(b"050").unwrap(),
            DecodedValue::InternalTemp(50.0)
        );
        assert_eq!(
            Decoder::Temp3dInt.decode(b"032").unwrap(),
            DecodedValue::InternalTemp(32.0)
        );
    }

    /// `tr` row — signal strength is 4 decimal digits 0000..1500.
    #[test]
    fn signal_strength_4d() {
        assert_eq!(
            Decoder::U16Dec4.decode(b"1500").unwrap(),
            DecodedValue::UInt(1500)
        );
        assert_eq!(
            Decoder::U16Dec4.decode(b"0000").unwrap(),
            DecodedValue::UInt(0)
        );
    }

    /// Per-1000 maxes: K = 1.200 → 1200, ε = 0.050 → 0050.
    #[test]
    fn per1000_round_trips_at_extremes() {
        assert_eq!(
            Decoder::U16DecMilli.decode(b"0050").unwrap(),
            DecodedValue::Per1000(0.050)
        );
        assert_eq!(
            Decoder::U16DecMilli.decode(b"1200").unwrap(),
            DecodedValue::Per1000(1.200)
        );
    }

    /// `ez` row — single digit, X = 0..6 (response time index).
    #[test]
    fn enum1_decodes_single_digit() {
        for x in 0..=6 {
            let payload = [b'0' + x as u8];
            assert_eq!(
                Decoder::Enum1.decode(&payload).unwrap(),
                DecodedValue::Enum(x as u8)
            );
        }
    }

    /// `lz` accepts wider 0..9 range (per manual: includes EXTERN,
    /// AUTO, HOLD as values 7/8/9).
    #[test]
    fn enum1_wide_accepts_full_range() {
        for x in 0..=9 {
            let payload = [b'0' + x as u8];
            assert_eq!(
                Decoder::Enum1Wide.decode(&payload).unwrap(),
                DecodedValue::Enum(x as u8)
            );
        }
    }

    /// Laser answer: "0" or "1".
    #[test]
    fn bool_digit_zero_one() {
        assert_eq!(
            Decoder::BoolDigit.decode(b"0").unwrap(),
            DecodedValue::Bool(false)
        );
        assert_eq!(
            Decoder::BoolDigit.decode(b"1").unwrap(),
            DecodedValue::Bool(true)
        );
    }

    /// Pure-write ack: ok / no.
    #[test]
    fn ack_round_trip() {
        assert_eq!(Decoder::Ack.decode(b"ok").unwrap(), DecodedValue::Ack(true));
        assert_eq!(Decoder::Ack.decode(b"no").unwrap(), DecodedValue::Ack(false));
    }

    /// `na` → 16 ASCII chars including trailing spaces. Manual:
    /// "Output: 'IGAR 6 Smart  ' (16 ASCII-characters)".
    #[test]
    fn device_type_text_payload() {
        let v = Decoder::Text.decode(b"IGAR 6 Smart  ").unwrap();
        assert_eq!(v, DecodedValue::Text("IGAR 6 Smart  ".into()));
    }

    /// `mb` / `me` row — 8 hex digits, "XXXXYYYY".
    #[test]
    fn basic_range_hex_pair() {
        let v = Decoder::HexPair8.decode(b"00FA09C4").unwrap();
        // 0x00FA = 250, 0x09C4 = 2500 — the IGAR 6 range in °C.
        assert_eq!(v, DecodedValue::HexPair { lo: 0x00FA, hi: 0x09C4 });
    }

    // ── Error paths ────────────────────────────────────────────────

    #[test]
    fn rejects_short_payload() {
        assert!(matches!(
            Decoder::U16DecMilli.decode(b"097"),
            Err(UppError::BadResponse(_))
        ));
        assert!(matches!(
            Decoder::Temp5dTenth.decode(b"1234"),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn rejects_long_payload() {
        assert!(matches!(
            Decoder::U16DecMilli.decode(b"09700"),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn rejects_non_digit() {
        assert!(matches!(
            Decoder::U16DecMilli.decode(b"09X0"),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn rejects_bad_ack() {
        assert!(matches!(
            Decoder::Ack.decode(b"xx"),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn rejects_bad_bool_digit() {
        assert!(matches!(
            Decoder::BoolDigit.decode(b"2"),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn rejects_bad_hex_pair() {
        assert!(matches!(
            Decoder::HexPair8.decode(b"00ZZ09C4"),
            Err(UppError::BadResponse(_))
        ));
    }
}
