//! UPP request encoder.
//!
//! Each [`Command`] variant maps to one row of manual §7 (pages
//! 31–33). [`Command::encode_request`] produces the exact byte
//! sequence the manual shows in the worked examples — verified by
//! the tests in this module.
//!
//! Wire grammar:
//!
//! ```text
//! request := AA cc [ param ] CR
//!   AA    = two ASCII decimal digits  (00..99)  — see `Address`
//!   cc    = two lowercase ASCII letters         — opcode mnemonic
//!   param = optional command-specific argument:
//!             empty            -> read this parameter
//!             4 ASCII digits   -> write a 4-digit value
//!             1 ASCII digit    -> write a 1-digit selector
//!             "?"              -> read the parameter's allowed range
//!             8 ASCII hex      -> write a sub-range pair (m1)
//!             ...              -> a few command-specific shapes
//!   CR    = 0x0D, ASCII 13
//! ```
//!
//! No checksum, no length field. Error detection is delegated to
//! UART parity + the master's 5 ms response timeout.

use crate::address::Address;
use crate::error::UppError;

/// Carriage Return — frame terminator (manual §7).
pub const CR: u8 = 0x0D;

/// One UPP request. Variants cover the full command table from
/// manual §7. The `Read*` / `Write*` distinction is explicit at the
/// type level so the client can decide whether to expect a response
/// without re-parsing the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    // ── 4-digit decimal parameters (the most common shape) ────────

    /// Read emissivity ε (returns 0050..1000 → 0.050..1.000).
    /// Manual §7 row "Emissivity ε for one-channel temperature" /
    /// command `em`.
    ReadEmissivity,
    /// Write emissivity ε. `value` is the 4-digit field 0050..1000.
    WriteEmissivity { value: u16 },

    /// Read transmittance τ (0050..1000).
    ReadTransmittance,
    /// Write transmittance τ. 0050..1000.
    WriteTransmittance { value: u16 },

    /// Read emissivity slope K = ε1 / ε2 (0800..1200 → 0.800..1.200).
    ReadEmissivityRatio,
    /// Write emissivity slope K. 0800..1200.
    WriteEmissivityRatio { value: u16 },

    /// Read "dirty window" warning threshold (XX hex, %).
    ReadDirtyWindow,
    /// Write "dirty window" warning threshold. `value` 0..=99.
    WriteDirtyWindow { value: u8 },

    /// Read minimum-intensity switch-off level (XX, 02..50 %).
    ReadSwitchOff,
    /// Write minimum-intensity switch-off level (02..=50).
    WriteSwitchOff { value: u8 },

    // ── 1-digit selectors / enums ────────────────────────────────

    /// Read response time t90 (X = 0..6).
    ReadResponseTime,
    /// Write response time t90. `value` 0..=6.
    WriteResponseTime { value: u8 },

    /// Read clear-peak-memory mode (X = 0..9).
    ReadClearPeak,
    /// Write clear-peak-memory mode. `value` 0..=9.
    WriteClearPeak { value: u8 },

    /// Read °C/°F display flag (0 = °C, 1 = °F).
    ReadFahrenheit,
    /// Write °C/°F display flag. `value` 0 or 1.
    WriteFahrenheit { value: u8 },

    /// Read operation mode (0=metal, 1=mono, 2=ratio, 3=Smart).
    ReadOpMode,
    /// Write operation mode. `value` 0..=3.
    WriteOpMode { value: u8 },

    /// Read laser-targeting state ("0" or "1").
    ReadLaser,
    /// Write laser-targeting state. `value` 0 or 1.
    WriteLaser { value: u8 },

    /// Read analog-output range (X = 0 → 0..20 mA, X = 1 → 4..20 mA).
    /// Manual lists this only as a write (`AAasX`), but we expose a
    /// "read" form via the limits-query mechanism (`?`) when needed.
    WriteAnalogOutput { value: u8 },

    /// Software simulation of an external clearance pulse (clears
    /// max-value storage). Manual §7 `lx` — pure-write, no parameter.
    SimulateClearPeak,

    // ── Multi-byte / hex parameters ──────────────────────────────

    /// Read device address (returns 2 digits, 00..99).
    ReadDeviceAddress,
    /// Write device address (2 digits, 00..99). The pyrometer accepts
    /// the new address even when broadcast 98 is used.
    WriteDeviceAddress { value: u8 },

    /// Read basic temperature range. Returns `XXXXYYYY` (4 hex
    /// digits low + 4 hex digits high, °C or °F).
    ReadBasicRange,
    /// Read currently active sub range (`XXXXYYYY`).
    ReadSubRange,
    /// Write a new sub range — step 1: send `m1` with the
    /// `XXXXYYYY` lo+hi pair. Manual §7 notes the change must be
    /// confirmed with `m2` (see [`Command::ConfirmSubRange`]).
    WriteSubRangeStep1 { lo_hex: u16, hi_hex: u16 },
    /// Write a new sub range — step 2: confirm. The pyrometer
    /// auto-resets if step 2 doesn't follow within its internal
    /// window.
    ConfirmSubRange,

    // ── Read-only measurements & metadata ────────────────────────

    /// Read measuring value (5 decimal digits, last is 1/10 °C/°F).
    ReadMeasuringValue,
    /// Read combined one-channel + ratio temperature
    /// (`SSSSSQQQQQ` — 2×5 digits).
    ReadMeasuringValuePair,
    /// Read peak-storage value (`AAtm`).
    ReadPeakValue,
    /// Read internal pyrometer temperature (3 decimal digits).
    ReadInternalTemp,
    /// Read relative signal strength (4 decimal digits, 0000..1500).
    ReadSignalStrength,
    /// Read serial number (5 hex digits).
    ReadSerialNumber,
    /// Read a 6-hex-digit reference number (`AAbn`).
    ReadReferenceNumber,
    /// Read device type — 16 ASCII characters, e.g. `"IGAR 6 Smart  "`.
    ReadDeviceType,
    /// Read all parameters in one packed answer (15 decimal digits).
    /// Layout per manual §7 row `pa`.
    ReadAllParameters,
    /// Read short device type / software version (`AAve` →
    /// `VVMMJJ`).
    ReadVersionShort,
    /// Read software version (detail) — `tt.mm.yy XX.YY`.
    ReadVersionDetailed,
    /// Read communication-module software version (detail) —
    /// `tt.mm.jj XX.YY`.
    ReadVersionCommModule,

    // ── Baud rate ────────────────────────────────────────────────

    /// Set baud rate. `value` is the manual's 0..=8 selector
    /// (mapping in [`BaudSelector`]).
    WriteBaudRate { value: u8 },

    // ── Limits query ─────────────────────────────────────────────

    /// Read the allowed range for a settable command, e.g.
    /// `00em?` → `00501000`. The wrapped command identifies which
    /// parameter to query — only the lowercase mnemonic is sent on
    /// the wire, so the `Read*` / `Write*` distinction is collapsed
    /// for this purpose.
    ReadLimits(LimitsTarget),
}

/// Settable parameters that support the `?` limits query.
/// Matches manual §7's "Example Read Limits Command" example
/// (`00em?` → `00501000` → ε ∈ 0.050..1.000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsTarget {
    Emissivity,
    Transmittance,
    EmissivityRatio,
    DirtyWindow,
    SwitchOff,
    ResponseTime,
    ClearPeak,
    OpMode,
    DeviceAddress,
    BaudRate,
}

impl LimitsTarget {
    /// 2-letter mnemonic on the wire.
    pub fn mnemonic(&self) -> &'static [u8; 2] {
        match self {
            LimitsTarget::Emissivity => b"em",
            LimitsTarget::Transmittance => b"et",
            LimitsTarget::EmissivityRatio => b"ev",
            LimitsTarget::DirtyWindow => b"dw",
            LimitsTarget::SwitchOff => b"aw",
            LimitsTarget::ResponseTime => b"ez",
            LimitsTarget::ClearPeak => b"lz",
            LimitsTarget::OpMode => b"ka",
            LimitsTarget::DeviceAddress => b"ga",
            LimitsTarget::BaudRate => b"br",
        }
    }
}

/// Mnemonic-only opcode lookup — returned to keep encoding logic
/// reusable between `Read*` / `Write*` branches and the `pa` packed
/// reader.
fn mnemonic(cmd: &Command) -> &'static [u8; 2] {
    use Command::*;
    match cmd {
        ReadEmissivity | WriteEmissivity { .. } => b"em",
        ReadTransmittance | WriteTransmittance { .. } => b"et",
        ReadEmissivityRatio | WriteEmissivityRatio { .. } => b"ev",
        ReadDirtyWindow | WriteDirtyWindow { .. } => b"dw",
        ReadSwitchOff | WriteSwitchOff { .. } => b"aw",
        ReadResponseTime | WriteResponseTime { .. } => b"ez",
        ReadClearPeak | WriteClearPeak { .. } => b"lz",
        ReadFahrenheit | WriteFahrenheit { .. } => b"fh",
        ReadOpMode | WriteOpMode { .. } => b"ka",
        ReadLaser | WriteLaser { .. } => b"la",
        WriteAnalogOutput { .. } => b"as",
        SimulateClearPeak => b"lx",
        ReadDeviceAddress | WriteDeviceAddress { .. } => b"ga",
        ReadBasicRange => b"mb",
        ReadSubRange => b"me",
        WriteSubRangeStep1 { .. } => b"m1",
        ConfirmSubRange => b"m2",
        ReadMeasuringValue => b"ms",
        ReadMeasuringValuePair => b"ek",
        ReadPeakValue => b"tm",
        ReadInternalTemp => b"gt",
        ReadSignalStrength => b"tr",
        ReadSerialNumber => b"sn",
        ReadReferenceNumber => b"bn",
        ReadDeviceType => b"na",
        ReadAllParameters => b"pa",
        ReadVersionShort => b"ve",
        ReadVersionDetailed => b"vs",
        ReadVersionCommModule => b"vc",
        WriteBaudRate { .. } => b"br",
        ReadLimits(t) => t.mnemonic(),
    }
}

impl Command {
    /// True if this command writes to the device (no response if
    /// addressed to broadcast 99).
    pub fn is_write(&self) -> bool {
        use Command::*;
        matches!(
            self,
            WriteEmissivity { .. }
                | WriteTransmittance { .. }
                | WriteEmissivityRatio { .. }
                | WriteDirtyWindow { .. }
                | WriteSwitchOff { .. }
                | WriteResponseTime { .. }
                | WriteClearPeak { .. }
                | WriteFahrenheit { .. }
                | WriteOpMode { .. }
                | WriteLaser { .. }
                | WriteAnalogOutput { .. }
                | SimulateClearPeak
                | WriteDeviceAddress { .. }
                | WriteSubRangeStep1 { .. }
                | ConfirmSubRange
                | WriteBaudRate { .. }
        )
    }

    /// Encode as the exact request byte sequence to send on the wire,
    /// including the trailing `CR`. Validates parameter ranges
    /// client-side.
    pub fn encode_request(&self, addr: Address) -> Result<Vec<u8>, UppError> {
        use Command::*;

        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&addr.encode());
        out.extend_from_slice(mnemonic(self));

        match self {
            // No-parameter reads
            ReadEmissivity
            | ReadTransmittance
            | ReadEmissivityRatio
            | ReadDirtyWindow
            | ReadSwitchOff
            | ReadResponseTime
            | ReadClearPeak
            | ReadFahrenheit
            | ReadOpMode
            | ReadLaser
            | ReadDeviceAddress
            | ReadBasicRange
            | ReadSubRange
            | ConfirmSubRange
            | ReadMeasuringValue
            | ReadMeasuringValuePair
            | ReadPeakValue
            | ReadInternalTemp
            | ReadSignalStrength
            | ReadSerialNumber
            | ReadReferenceNumber
            | ReadDeviceType
            | ReadAllParameters
            | ReadVersionShort
            | ReadVersionDetailed
            | ReadVersionCommModule
            | SimulateClearPeak => {}

            // 4-digit decimal writes
            WriteEmissivity { value } => append_4digit(&mut out, *value, 50, 1000, "emissivity")?,
            WriteTransmittance { value } => {
                append_4digit(&mut out, *value, 50, 1000, "transmittance")?
            }
            WriteEmissivityRatio { value } => {
                append_4digit(&mut out, *value, 800, 1200, "emissivity-ratio K")?
            }

            // 2-digit decimal writes (printed as 4 digits in the
            // manual? actually the table shows XX = 02..50 / 00..99
            // hex — they're 2-digit, but our representation matches
            // the manual exactly).
            WriteDirtyWindow { value } => append_2digit_hex(&mut out, *value, 0, 99, "dirty-window")?,
            WriteSwitchOff { value } => append_2digit_dec(&mut out, *value, 2, 50, "switch-off")?,

            // 1-digit selectors
            WriteResponseTime { value } => append_1digit(&mut out, *value, 0, 6, "response-time")?,
            WriteClearPeak { value } => append_1digit(&mut out, *value, 0, 9, "clear-peak")?,
            WriteFahrenheit { value } => append_1digit(&mut out, *value, 0, 1, "fahrenheit")?,
            WriteOpMode { value } => append_1digit(&mut out, *value, 0, 3, "op-mode")?,
            WriteLaser { value } => append_1digit(&mut out, *value, 0, 1, "laser")?,
            WriteAnalogOutput { value } => append_1digit(&mut out, *value, 0, 1, "analog-output")?,
            WriteBaudRate { value } => {
                // Manual: 7 is "not allowed". Reject it client-side
                // so the bus is never wasted.
                if *value == 7 {
                    return Err(UppError::OutOfRange(
                        "baud-rate selector 7 is not allowed (manual §7)".into(),
                    ));
                }
                append_1digit(&mut out, *value, 0, 8, "baud-rate")?
            }

            // 2-digit decimal device-address write
            WriteDeviceAddress { value } => {
                if *value > 99 {
                    return Err(UppError::OutOfRange(format!(
                        "device-address must be 00..=99, got {value}"
                    )));
                }
                out.push(b'0' + value / 10);
                out.push(b'0' + value % 10);
            }

            // 8-hex-digit sub-range write (XXXXYYYY)
            WriteSubRangeStep1 { lo_hex, hi_hex } => {
                append_4hex(&mut out, *lo_hex);
                append_4hex(&mut out, *hi_hex);
            }

            // Limits query — same mnemonic as the parameter, then `?`
            ReadLimits(_) => out.push(b'?'),
        }

        out.push(CR);
        Ok(out)
    }
}

// ── Encoding helpers (range-checked) ───────────────────────────────

fn append_4digit(out: &mut Vec<u8>, v: u16, lo: u16, hi: u16, what: &str) -> Result<(), UppError> {
    if v < lo || v > hi {
        return Err(UppError::OutOfRange(format!(
            "{what} must be {lo:04}..={hi:04}, got {v}"
        )));
    }
    let s = format!("{v:04}");
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn append_2digit_dec(out: &mut Vec<u8>, v: u8, lo: u8, hi: u8, what: &str) -> Result<(), UppError> {
    if v < lo || v > hi {
        return Err(UppError::OutOfRange(format!(
            "{what} must be {lo:02}..={hi:02}, got {v}"
        )));
    }
    out.push(b'0' + v / 10);
    out.push(b'0' + v % 10);
    Ok(())
}

fn append_2digit_hex(out: &mut Vec<u8>, v: u8, lo: u8, hi: u8, what: &str) -> Result<(), UppError> {
    if v < lo || v > hi {
        return Err(UppError::OutOfRange(format!(
            "{what} must be {lo:02}..={hi:02}, got {v}"
        )));
    }
    let s = format!("{v:02X}");
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn append_1digit(out: &mut Vec<u8>, v: u8, lo: u8, hi: u8, what: &str) -> Result<(), UppError> {
    if v < lo || v > hi {
        return Err(UppError::OutOfRange(format!(
            "{what} must be {lo}..={hi}, got {v}"
        )));
    }
    out.push(b'0' + v);
    Ok(())
}

fn append_4hex(out: &mut Vec<u8>, v: u16) {
    let s = format!("{v:04X}");
    out.extend_from_slice(s.as_bytes());
}

// ── Spec tests ─────────────────────────────────────────────────────
//
// These reproduce the worked examples from manual §7 byte-for-byte.
// They are the protocol's executable specification: a regression in
// the encoder breaks the manual contract, and that is what they pin.

#[cfg(test)]
mod tests {
    use super::*;

    fn ind(n: u8) -> Address {
        Address::individual(n).unwrap()
    }

    /// Manual §7 example: "Read Command: Entry: '00em' + CR".
    #[test]
    fn manual_example_read_emissivity() {
        let bytes = Command::ReadEmissivity.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00em\r");
    }

    /// Manual §7 example: "Write Command: Entry: '00emXXXX' + CR" /
    /// "Answer: '00em0853' + CR changes the Emissivity to 0.853".
    /// We pin the request side: writing 0853 emits exactly `00em0853\r`.
    #[test]
    fn manual_example_write_emissivity_0853() {
        let bytes = Command::WriteEmissivity { value: 853 }
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes, b"00em0853\r");
    }

    /// Manual §7 example: "Read Limits Command: Entry: '00em?' + CR".
    #[test]
    fn manual_example_read_limits_emissivity() {
        let bytes = Command::ReadLimits(LimitsTarget::Emissivity)
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes, b"00em?\r");
    }

    /// Manual §7 row for `ka`: "X = 0 metal mode, 1 mono, 2 ratio,
    /// 3 Smart". Worked example would be `00ka2` to set ratio mode
    /// on device 00.
    #[test]
    fn write_op_mode_emits_one_digit() {
        let bytes = Command::WriteOpMode { value: 2 }
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes, b"00ka2\r");
    }

    /// Cover the address-prefix encoding: same command, address 42.
    #[test]
    fn address_prefix_pads_to_two_digits() {
        let bytes = Command::ReadEmissivity.encode_request(ind(42)).unwrap();
        assert_eq!(bytes, b"42em\r");
    }

    /// Broadcast write for parameters (manual §4.14: address 99
    /// "global address without response").
    #[test]
    fn broadcast_write_no_response_address_99() {
        let bytes = Command::WriteEmissivity { value: 853 }
            .encode_request(Address::BroadcastNoResponse)
            .unwrap();
        assert_eq!(bytes, b"99em0853\r");
    }

    #[test]
    fn write_emissivity_rejects_out_of_range_low() {
        // Manual: ε ∈ 0050..1000.
        let err = Command::WriteEmissivity { value: 49 }
            .encode_request(ind(0))
            .unwrap_err();
        assert!(matches!(err, UppError::OutOfRange(_)), "got {err:?}");
    }

    #[test]
    fn write_emissivity_rejects_out_of_range_high() {
        let err = Command::WriteEmissivity { value: 1001 }
            .encode_request(ind(0))
            .unwrap_err();
        assert!(matches!(err, UppError::OutOfRange(_)));
    }

    #[test]
    fn write_emissivity_ratio_full_range() {
        // Manual: K ∈ 0800..1200.
        assert!(Command::WriteEmissivityRatio { value: 800 }
            .encode_request(ind(0))
            .is_ok());
        assert!(Command::WriteEmissivityRatio { value: 1200 }
            .encode_request(ind(0))
            .is_ok());
        assert!(Command::WriteEmissivityRatio { value: 799 }
            .encode_request(ind(0))
            .is_err());
        assert!(Command::WriteEmissivityRatio { value: 1201 }
            .encode_request(ind(0))
            .is_err());
    }

    #[test]
    fn write_baud_rate_rejects_selector_7() {
        // Manual lists "7 = is not allowed".
        let err = Command::WriteBaudRate { value: 7 }
            .encode_request(ind(0))
            .unwrap_err();
        assert!(matches!(err, UppError::OutOfRange(_)));
    }

    #[test]
    fn write_baud_rate_accepts_8_for_115200() {
        // Manual: 8 = 115200 Baud.
        let bytes = Command::WriteBaudRate { value: 8 }
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes, b"00br8\r");
    }

    #[test]
    fn read_measuring_value() {
        // Manual `ms` row → 5-digit answer; request side is `AAms`.
        let bytes = Command::ReadMeasuringValue
            .encode_request(ind(7))
            .unwrap();
        assert_eq!(bytes, b"07ms\r");
    }

    #[test]
    fn read_measuring_pair() {
        // `ek` returns 1-channel + ratio temperature in 10 digits.
        let bytes = Command::ReadMeasuringValuePair
            .encode_request(ind(7))
            .unwrap();
        assert_eq!(bytes, b"07ek\r");
    }

    #[test]
    fn read_internal_temp() {
        let bytes = Command::ReadInternalTemp.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00gt\r");
    }

    #[test]
    fn read_device_type() {
        let bytes = Command::ReadDeviceType.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00na\r");
    }

    #[test]
    fn read_all_parameters() {
        let bytes = Command::ReadAllParameters.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00pa\r");
    }

    #[test]
    fn write_sub_range_step1_pair_hex() {
        // Manual `m1XXXXYYYY` — example: lo = 925 (0x039D), hi = 975
        // (0x03CF). The exact hex pair the user would send is
        // "039D03CF". The choice of hex digits is implementation
        // detail; we just pin the FORMAT (4 hex digits per side).
        let bytes = Command::WriteSubRangeStep1 {
            lo_hex: 0x039D,
            hi_hex: 0x03CF,
        }
        .encode_request(ind(0))
        .unwrap();
        assert_eq!(bytes, b"00m1039D03CF\r");
    }

    #[test]
    fn confirm_sub_range_no_payload() {
        let bytes = Command::ConfirmSubRange.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00m2\r");
    }

    #[test]
    fn simulate_clear_peak_no_payload() {
        let bytes = Command::SimulateClearPeak.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00lx\r");
    }

    #[test]
    fn read_device_address_via_ga() {
        let bytes = Command::ReadDeviceAddress.encode_request(ind(0)).unwrap();
        assert_eq!(bytes, b"00ga\r");
    }

    #[test]
    fn write_device_address_two_digits() {
        let bytes = Command::WriteDeviceAddress { value: 42 }
            .encode_request(Address::BroadcastWithResponse)
            .unwrap();
        assert_eq!(bytes, b"98ga42\r");
    }

    #[test]
    fn dirty_window_is_two_hex_digits() {
        // Manual: AAdwXX where XX is hex. `99 dec` = `63 hex`.
        let bytes = Command::WriteDirtyWindow { value: 99 }
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes, b"00dw63\r");
        // 0% (off, factory default).
        let bytes0 = Command::WriteDirtyWindow { value: 0 }
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes0, b"00dw00\r");
    }

    #[test]
    fn switch_off_is_two_dec_digits_in_range() {
        let bytes = Command::WriteSwitchOff { value: 10 }
            .encode_request(ind(0))
            .unwrap();
        assert_eq!(bytes, b"00aw10\r");
        // Below the manual minimum (02) is rejected.
        let err = Command::WriteSwitchOff { value: 1 }
            .encode_request(ind(0))
            .unwrap_err();
        assert!(matches!(err, UppError::OutOfRange(_)));
    }

    #[test]
    fn is_write_classifies_correctly() {
        assert!(Command::WriteEmissivity { value: 500 }.is_write());
        assert!(Command::SimulateClearPeak.is_write());
        assert!(Command::ConfirmSubRange.is_write());
        assert!(!Command::ReadEmissivity.is_write());
        assert!(!Command::ReadMeasuringValue.is_write());
        assert!(!Command::ReadLimits(LimitsTarget::Emissivity).is_write());
    }

    /// Every Command variant must produce a request that ends in CR
    /// and starts with the right address prefix. This sweeps the
    /// table so a future variant additions can't accidentally drop
    /// the terminator.
    #[test]
    fn every_variant_terminates_in_cr_and_has_address_prefix() {
        let variants = [
            Command::ReadEmissivity,
            Command::WriteEmissivity { value: 500 },
            Command::ReadTransmittance,
            Command::WriteTransmittance { value: 500 },
            Command::ReadEmissivityRatio,
            Command::WriteEmissivityRatio { value: 1000 },
            Command::ReadDirtyWindow,
            Command::WriteDirtyWindow { value: 50 },
            Command::ReadSwitchOff,
            Command::WriteSwitchOff { value: 10 },
            Command::ReadResponseTime,
            Command::WriteResponseTime { value: 0 },
            Command::ReadClearPeak,
            Command::WriteClearPeak { value: 0 },
            Command::ReadFahrenheit,
            Command::WriteFahrenheit { value: 0 },
            Command::ReadOpMode,
            Command::WriteOpMode { value: 2 },
            Command::ReadLaser,
            Command::WriteLaser { value: 0 },
            Command::WriteAnalogOutput { value: 1 },
            Command::SimulateClearPeak,
            Command::ReadDeviceAddress,
            Command::WriteDeviceAddress { value: 0 },
            Command::ReadBasicRange,
            Command::ReadSubRange,
            Command::WriteSubRangeStep1 { lo_hex: 0, hi_hex: 0 },
            Command::ConfirmSubRange,
            Command::ReadMeasuringValue,
            Command::ReadMeasuringValuePair,
            Command::ReadPeakValue,
            Command::ReadInternalTemp,
            Command::ReadSignalStrength,
            Command::ReadSerialNumber,
            Command::ReadReferenceNumber,
            Command::ReadDeviceType,
            Command::ReadAllParameters,
            Command::ReadVersionShort,
            Command::ReadVersionDetailed,
            Command::ReadVersionCommModule,
            Command::WriteBaudRate { value: 4 },
            Command::ReadLimits(LimitsTarget::Emissivity),
            Command::ReadLimits(LimitsTarget::BaudRate),
        ];

        for cmd in variants {
            let bytes = cmd
                .encode_request(ind(0))
                .unwrap_or_else(|e| panic!("{cmd:?} failed: {e}"));
            assert!(
                bytes.starts_with(b"00"),
                "{cmd:?} → {:?} missing AA prefix",
                String::from_utf8_lossy(&bytes)
            );
            assert_eq!(
                *bytes.last().unwrap(),
                CR,
                "{cmd:?} → {:?} not CR-terminated",
                String::from_utf8_lossy(&bytes)
            );
        }
    }
}
