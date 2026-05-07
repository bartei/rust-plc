//! Resolve a YAML `upp:` block into a typed [`ResolvedBinding`].
//!
//! The profile YAML carries plain strings (`command: "ms"`,
//! `decoder: "temp_5d_tenth"`) so [`st_comm_api`] does not have to
//! depend on this crate. At FB construction time we resolve those
//! strings into the typed [`Command`](crate::Command) and
//! [`Decoder`](crate::Decoder) variants — typos and unknown opcodes
//! are surfaced as [`UppError::BadResponse`] before the bus is ever
//! touched.

use crate::command::{Command, LimitsTarget};
use crate::error::UppError;
use crate::parser::{Decoder, MeasurementChannel};
use st_comm_api::profile::{ProfileField, UppFieldBinding};

/// Per-field binding the FB driver actually uses.
#[derive(Debug, Clone)]
pub struct ResolvedBinding {
    /// Read command (always issued each poll cycle for input fields).
    pub read_cmd: Command,
    /// Optional write command — present only when the field's
    /// direction is `output` or `inout`. The mnemonic is the same as
    /// the read command but the variant carries a placeholder zero
    /// value; the driver supplies the actual numeric value at write
    /// time.
    pub write_cmd_kind: Option<WriteCmdKind>,
    /// Decoder for the response payload.
    pub decoder: Decoder,
}

/// Identifies how to construct a write command at runtime. The
/// runtime supplies the actual numeric value when it issues the
/// write — this enum just records which mnemonic shape to build.
#[derive(Debug, Clone, Copy)]
pub enum WriteCmdKind {
    Em,
    Et,
    Ev,
    Dw,
    Aw,
    Ez,
    Lz,
    Fh,
    Ka,
    La,
    As,
    Ga,
    Br,
    /// Sub-range write step 1 (8-hex-digit pair).
    M1,
    /// Sub-range confirm — no parameter; matches `ConfirmSubRange`.
    M2,
    /// Pure-write trigger (`lx`) — no parameter.
    Lx,
}

/// Resolve a YAML binding string pair into the typed enums.
pub fn resolve(field: &ProfileField) -> Result<ResolvedBinding, UppError> {
    let upp = field.upp.as_ref().ok_or_else(|| {
        UppError::BadResponse(format!(
            "field {:?} has no `upp:` binding — cannot route through UPP protocol",
            field.name
        ))
    })?;

    let read_cmd = resolve_read_command(upp)?;
    let write_cmd_kind = match field.direction {
        st_comm_api::profile::FieldDirection::Output
        | st_comm_api::profile::FieldDirection::Inout => {
            Some(resolve_write_kind(&upp.command).ok_or_else(|| {
                UppError::BadResponse(format!(
                    "field {:?}: command {:?} is not writable",
                    field.name, upp.command
                ))
            })?)
        }
        _ => None,
    };
    let decoder = resolve_decoder(upp)?;

    Ok(ResolvedBinding { read_cmd, write_cmd_kind, decoder })
}

fn resolve_read_command(b: &UppFieldBinding) -> Result<Command, UppError> {
    use Command::*;
    let cmd = b.command.as_str();
    Ok(match cmd {
        "em" => ReadEmissivity,
        "et" => ReadTransmittance,
        "ev" => ReadEmissivityRatio,
        "dw" => ReadDirtyWindow,
        "aw" => ReadSwitchOff,
        "ez" => ReadResponseTime,
        "lz" => ReadClearPeak,
        "fh" => ReadFahrenheit,
        "ka" => ReadOpMode,
        "la" => ReadLaser,
        "ga" => ReadDeviceAddress,
        "mb" => ReadBasicRange,
        "me" => ReadSubRange,
        "ms" => ReadMeasuringValue,
        "ek" => ReadMeasuringValuePair,
        "tm" => ReadPeakValue,
        "gt" => ReadInternalTemp,
        "tr" => ReadSignalStrength,
        "sn" => ReadSerialNumber,
        "bn" => ReadReferenceNumber,
        "na" => ReadDeviceType,
        "pa" => ReadAllParameters,
        "ve" => ReadVersionShort,
        "vs" => ReadVersionDetailed,
        "vc" => ReadVersionCommModule,
        // Limits queries are written as `?em` etc. in the YAML — but
        // we're more pragmatic: any binding tagged with the special
        // sentinel mnemonic `"<param>?"` resolves to ReadLimits.
        // Documented in design_igar.md.
        s if s.ends_with('?') => ReadLimits(resolve_limits_target(&s[..s.len() - 1])?),
        other => {
            return Err(UppError::BadResponse(format!(
                "unknown UPP command mnemonic {other:?}"
            )))
        }
    })
}

fn resolve_limits_target(mnem: &str) -> Result<LimitsTarget, UppError> {
    Ok(match mnem {
        "em" => LimitsTarget::Emissivity,
        "et" => LimitsTarget::Transmittance,
        "ev" => LimitsTarget::EmissivityRatio,
        "dw" => LimitsTarget::DirtyWindow,
        "aw" => LimitsTarget::SwitchOff,
        "ez" => LimitsTarget::ResponseTime,
        "lz" => LimitsTarget::ClearPeak,
        "ka" => LimitsTarget::OpMode,
        "ga" => LimitsTarget::DeviceAddress,
        "br" => LimitsTarget::BaudRate,
        other => {
            return Err(UppError::BadResponse(format!(
                "no limits-query mapping for mnemonic {other:?}"
            )))
        }
    })
}

fn resolve_write_kind(mnem: &str) -> Option<WriteCmdKind> {
    Some(match mnem {
        "em" => WriteCmdKind::Em,
        "et" => WriteCmdKind::Et,
        "ev" => WriteCmdKind::Ev,
        "dw" => WriteCmdKind::Dw,
        "aw" => WriteCmdKind::Aw,
        "ez" => WriteCmdKind::Ez,
        "lz" => WriteCmdKind::Lz,
        "fh" => WriteCmdKind::Fh,
        "ka" => WriteCmdKind::Ka,
        "la" => WriteCmdKind::La,
        "as" => WriteCmdKind::As,
        "ga" => WriteCmdKind::Ga,
        "br" => WriteCmdKind::Br,
        "m1" => WriteCmdKind::M1,
        "m2" => WriteCmdKind::M2,
        "lx" => WriteCmdKind::Lx,
        _ => return None,
    })
}

fn resolve_decoder(b: &UppFieldBinding) -> Result<Decoder, UppError> {
    let dec = b.decoder.as_str();
    Ok(match dec {
        "temp_5d_tenth" => Decoder::Temp5dTenth,
        "temp_pair_5d_tenth" => {
            let ch = b
                .channel
                .as_deref()
                .ok_or_else(|| {
                    UppError::BadResponse(
                        "decoder temp_pair_5d_tenth requires channel: one_channel | ratio".into(),
                    )
                })?;
            let channel = match ch {
                "one_channel" => MeasurementChannel::OneChannel,
                "ratio" => MeasurementChannel::Ratio,
                other => {
                    return Err(UppError::BadResponse(format!(
                        "unknown channel selector {other:?} (must be one_channel or ratio)"
                    )))
                }
            };
            Decoder::TempPair5dTenth { channel }
        }
        "temp_3d_int" => Decoder::Temp3dInt,
        "u16_dec_4" | "u16_dec" => Decoder::U16Dec4,
        "u16_dec_milli" => Decoder::U16DecMilli,
        "enum_1" | "enum_response_time" | "enum_op_mode" => Decoder::Enum1,
        "enum_1_wide" | "enum_clear_peak" => Decoder::Enum1Wide,
        "bool_digit" => Decoder::BoolDigit,
        "hex_pair_8" | "range_pair" => Decoder::HexPair8,
        "ack" => Decoder::Ack,
        "text" => Decoder::Text,
        other => {
            return Err(UppError::BadResponse(format!(
                "unknown UPP decoder name {other:?}"
            )))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use st_comm_api::profile::{FieldDataType, FieldDirection};

    fn field(
        name: &str,
        dir: FieldDirection,
        cmd: &str,
        dec: &str,
        ch: Option<&str>,
    ) -> ProfileField {
        ProfileField {
            name: name.into(),
            data_type: FieldDataType::Real,
            direction: dir,
            register: None,
            upp: Some(UppFieldBinding {
                command: cmd.into(),
                decoder: dec.into(),
                channel: ch.map(String::from),
            }),
            count: 1,
            description: None,
        }
    }

    #[test]
    fn resolves_read_only_temperature() {
        let b = resolve(&field("t", FieldDirection::Input, "ms", "temp_5d_tenth", None)).unwrap();
        assert!(matches!(b.read_cmd, Command::ReadMeasuringValue));
        assert!(b.write_cmd_kind.is_none());
        assert_eq!(b.decoder, Decoder::Temp5dTenth);
    }

    #[test]
    fn resolves_inout_emissivity() {
        let b = resolve(&field("e", FieldDirection::Inout, "em", "u16_dec_milli", None)).unwrap();
        assert!(matches!(b.read_cmd, Command::ReadEmissivity));
        assert!(matches!(b.write_cmd_kind, Some(WriteCmdKind::Em)));
        assert_eq!(b.decoder, Decoder::U16DecMilli);
    }

    #[test]
    fn resolves_ek_with_channel_ratio() {
        let b = resolve(&field(
            "rt",
            FieldDirection::Input,
            "ek",
            "temp_pair_5d_tenth",
            Some("ratio"),
        ))
        .unwrap();
        assert!(matches!(b.read_cmd, Command::ReadMeasuringValuePair));
        assert_eq!(
            b.decoder,
            Decoder::TempPair5dTenth { channel: MeasurementChannel::Ratio }
        );
    }

    #[test]
    fn rejects_ek_without_channel() {
        let err = resolve(&field(
            "rt",
            FieldDirection::Input,
            "ek",
            "temp_pair_5d_tenth",
            None,
        ))
        .unwrap_err();
        assert!(matches!(err, UppError::BadResponse(_)));
    }

    #[test]
    fn rejects_unknown_command() {
        let err =
            resolve(&field("x", FieldDirection::Input, "zz", "temp_5d_tenth", None)).unwrap_err();
        assert!(matches!(err, UppError::BadResponse(_)));
    }

    #[test]
    fn rejects_unknown_decoder() {
        let err =
            resolve(&field("x", FieldDirection::Input, "em", "no_such_decoder", None))
                .unwrap_err();
        assert!(matches!(err, UppError::BadResponse(_)));
    }

    #[test]
    fn rejects_field_without_upp_binding() {
        let f = ProfileField {
            name: "x".into(),
            data_type: FieldDataType::Real,
            direction: FieldDirection::Input,
            register: None,
            upp: None,
            count: 1,
            description: None,
        };
        assert!(matches!(resolve(&f), Err(UppError::BadResponse(_))));
    }

    #[test]
    fn limits_query_via_question_mark_suffix() {
        // The reference profile uses "em?" to mean "read-limits of em".
        let b = resolve(&field("lim", FieldDirection::Input, "em?", "hex_pair_8", None))
            .unwrap();
        match b.read_cmd {
            Command::ReadLimits(LimitsTarget::Emissivity) => {}
            other => panic!("expected ReadLimits(Emissivity), got {other:?}"),
        }
    }

    #[test]
    fn rejects_write_on_read_only_command() {
        // `ms` (measuring value) is a read-only opcode. Inout
        // direction should error.
        let err = resolve(&field("t", FieldDirection::Inout, "ms", "temp_5d_tenth", None))
            .unwrap_err();
        assert!(matches!(err, UppError::BadResponse(_)));
    }
}
