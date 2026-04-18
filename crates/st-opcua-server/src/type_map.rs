//! IEC 61131-3 to OPC-UA type mapping and value conversion.
//!
//! Converts between the PLC runtime's string-based value representation
//! and OPC-UA typed Variants. The string format matches `format_value()`
//! in `st-engine/src/debug.rs`.

use opcua_types::{DataTypeId, Variant};

/// Map an IEC 61131-3 type name to an OPC-UA DataType node ID.
///
/// Returns `None` for unrecognized types (they will be exposed as String).
pub fn iec_type_to_opcua_data_type(iec_type: &str) -> DataTypeId {
    match iec_type.to_uppercase().as_str() {
        "BOOL" => DataTypeId::Boolean,
        "SINT" => DataTypeId::SByte,
        "INT" => DataTypeId::Int16,
        "DINT" => DataTypeId::Int32,
        "LINT" => DataTypeId::Int64,
        "USINT" => DataTypeId::Byte,
        "UINT" => DataTypeId::UInt16,
        "UDINT" => DataTypeId::UInt32,
        "ULINT" => DataTypeId::UInt64,
        "REAL" => DataTypeId::Float,
        "LREAL" => DataTypeId::Double,
        "STRING" => DataTypeId::String,
        "TIME" => DataTypeId::Int64, // milliseconds
        "BYTE" => DataTypeId::Byte,
        "WORD" => DataTypeId::UInt16,
        "DWORD" => DataTypeId::UInt32,
        "LWORD" => DataTypeId::UInt64,
        _ => DataTypeId::String, // fallback: expose unknown types as string
    }
}

/// Parse a PLC value string into an OPC-UA Variant, guided by the IEC type.
///
/// The string format matches `st-engine/src/debug.rs:format_value()`:
/// - Bool: `"TRUE"` / `"FALSE"`
/// - Int/UInt: decimal `"42"`, `"-7"`
/// - Real: `"3.140000"` (6 decimal places)
/// - String: `"'hello'"` (single-quoted)
/// - Time: `"T#500ms"`, `"T#1s500ms"`
pub fn parse_value_to_variant(value: &str, iec_type: &str) -> Variant {
    match iec_type.to_uppercase().as_str() {
        "BOOL" => parse_bool(value),
        "SINT" => parse_sint(value),
        "INT" => parse_int16(value),
        "DINT" => parse_int32(value),
        "LINT" => parse_int64(value),
        "USINT" | "BYTE" => parse_byte(value),
        "UINT" | "WORD" => parse_uint16(value),
        "UDINT" | "DWORD" => parse_uint32(value),
        "ULINT" | "LWORD" => parse_uint64(value),
        "REAL" => parse_float(value),
        "LREAL" => parse_double(value),
        "STRING" => parse_string(value),
        "TIME" => parse_time(value),
        _ => Variant::String(value.to_string().into()),
    }
}

/// Convert an OPC-UA Variant back to a PLC value string for force_variable().
///
/// This is the reverse of `parse_value_to_variant`. The output must be
/// parseable by `runtime_manager::parse_value_string()`.
pub fn variant_to_value_string(variant: &Variant) -> String {
    match variant {
        Variant::Boolean(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Variant::SByte(v) => v.to_string(),
        Variant::Int16(v) => v.to_string(),
        Variant::Int32(v) => v.to_string(),
        Variant::Int64(v) => v.to_string(),
        Variant::Byte(v) => v.to_string(),
        Variant::UInt16(v) => v.to_string(),
        Variant::UInt32(v) => v.to_string(),
        Variant::UInt64(v) => v.to_string(),
        Variant::Float(v) => format!("{v}"),
        Variant::Double(v) => format!("{v}"),
        Variant::String(s) => s.as_ref().to_string(),
        _ => format!("{variant:?}"),
    }
}

// ── Individual type parsers ────────────────────────────────────────────

fn parse_bool(value: &str) -> Variant {
    Variant::Boolean(value.eq_ignore_ascii_case("true") || value == "1")
}

fn parse_sint(value: &str) -> Variant {
    let i = value.parse::<i64>().unwrap_or(0);
    Variant::SByte(i as i8)
}

fn parse_int16(value: &str) -> Variant {
    let i = value.parse::<i64>().unwrap_or(0);
    Variant::Int16(i as i16)
}

fn parse_int32(value: &str) -> Variant {
    let i = value.parse::<i64>().unwrap_or(0);
    Variant::Int32(i as i32)
}

fn parse_int64(value: &str) -> Variant {
    let i = value.parse::<i64>().unwrap_or(0);
    Variant::Int64(i)
}

fn parse_byte(value: &str) -> Variant {
    let u = value.parse::<u64>().unwrap_or(0);
    Variant::Byte(u as u8)
}

fn parse_uint16(value: &str) -> Variant {
    let u = value.parse::<u64>().unwrap_or(0);
    Variant::UInt16(u as u16)
}

fn parse_uint32(value: &str) -> Variant {
    let u = value.parse::<u64>().unwrap_or(0);
    Variant::UInt32(u as u32)
}

fn parse_uint64(value: &str) -> Variant {
    let u = value.parse::<u64>().unwrap_or(0);
    Variant::UInt64(u)
}

fn parse_float(value: &str) -> Variant {
    let f = value.parse::<f64>().unwrap_or(0.0);
    Variant::Float(f as f32)
}

fn parse_double(value: &str) -> Variant {
    let f = value.parse::<f64>().unwrap_or(0.0);
    Variant::Double(f)
}

fn parse_string(value: &str) -> Variant {
    // The PLC formats strings as 'hello' (single-quoted). Strip the quotes.
    let stripped = if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        &value[1..value.len() - 1]
    } else {
        value
    };
    Variant::String(stripped.to_string().into())
}

fn parse_time(value: &str) -> Variant {
    // TIME values are formatted as "T#..." or just milliseconds.
    // Parse common formats: "T#500ms", "T#1s", "T#1s500ms", "T#1m", "500"
    let ms = parse_time_string(value);
    Variant::Int64(ms)
}

/// Parse a TIME literal string to milliseconds.
///
/// Supports: `"T#500ms"`, `"T#1s"`, `"T#1s500ms"`, `"T#2m30s"`, bare `"500"`.
fn parse_time_string(s: &str) -> i64 {
    // Strip T# prefix
    let s = s.strip_prefix("T#").or_else(|| s.strip_prefix("t#")).unwrap_or(s);

    // Bare number = milliseconds
    if let Ok(ms) = s.parse::<i64>() {
        return ms;
    }

    let mut total_ms: i64 = 0;
    let mut num_buf = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_digit() || ch == '.' {
            num_buf.push(ch);
            i += 1;
        } else {
            let num: f64 = num_buf.parse().unwrap_or(0.0);
            num_buf.clear();
            match ch {
                'h' => {
                    total_ms += (num * 3_600_000.0) as i64;
                    i += 1;
                }
                'm' => {
                    // Check if this is 'ms' (milliseconds) or 'm' (minutes)
                    if i + 1 < chars.len() && chars[i + 1] == 's' {
                        total_ms += num as i64; // milliseconds
                        i += 2; // skip 'm' and 's'
                    } else {
                        total_ms += (num * 60_000.0) as i64; // minutes
                        i += 1;
                    }
                }
                's' => {
                    total_ms += (num * 1_000.0) as i64;
                    i += 1;
                }
                _ => {
                    i += 1;
                }
            }
        }
    }

    total_ms
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Type mapping tests ─────────────────────────────────────────

    #[test]
    fn iec_type_mapping() {
        assert_eq!(iec_type_to_opcua_data_type("BOOL"), DataTypeId::Boolean);
        assert_eq!(iec_type_to_opcua_data_type("INT"), DataTypeId::Int16);
        assert_eq!(iec_type_to_opcua_data_type("DINT"), DataTypeId::Int32);
        assert_eq!(iec_type_to_opcua_data_type("REAL"), DataTypeId::Float);
        assert_eq!(iec_type_to_opcua_data_type("LREAL"), DataTypeId::Double);
        assert_eq!(iec_type_to_opcua_data_type("STRING"), DataTypeId::String);
        assert_eq!(iec_type_to_opcua_data_type("TIME"), DataTypeId::Int64);
        assert_eq!(iec_type_to_opcua_data_type("BYTE"), DataTypeId::Byte);
        assert_eq!(iec_type_to_opcua_data_type("WORD"), DataTypeId::UInt16);
        assert_eq!(iec_type_to_opcua_data_type("DWORD"), DataTypeId::UInt32);
        assert_eq!(iec_type_to_opcua_data_type("LWORD"), DataTypeId::UInt64);
    }

    #[test]
    fn iec_type_mapping_case_insensitive() {
        assert_eq!(iec_type_to_opcua_data_type("bool"), DataTypeId::Boolean);
        assert_eq!(iec_type_to_opcua_data_type("Int"), DataTypeId::Int16);
        assert_eq!(iec_type_to_opcua_data_type("rEaL"), DataTypeId::Float);
    }

    #[test]
    fn unknown_type_maps_to_string() {
        assert_eq!(
            iec_type_to_opcua_data_type("UNKNOWN"),
            DataTypeId::String
        );
    }

    // ── Boolean parsing ────────────────────────────────────────────

    #[test]
    fn parse_bool_true() {
        assert_eq!(parse_value_to_variant("TRUE", "BOOL"), Variant::Boolean(true));
        assert_eq!(parse_value_to_variant("true", "BOOL"), Variant::Boolean(true));
        assert_eq!(parse_value_to_variant("1", "BOOL"), Variant::Boolean(true));
    }

    #[test]
    fn parse_bool_false() {
        assert_eq!(
            parse_value_to_variant("FALSE", "BOOL"),
            Variant::Boolean(false)
        );
        assert_eq!(
            parse_value_to_variant("0", "BOOL"),
            Variant::Boolean(false)
        );
    }

    // ── Integer parsing ────────────────────────────────────────────

    #[test]
    fn parse_sint() {
        assert_eq!(parse_value_to_variant("42", "SINT"), Variant::SByte(42));
        assert_eq!(parse_value_to_variant("-7", "SINT"), Variant::SByte(-7));
        assert_eq!(parse_value_to_variant("0", "SINT"), Variant::SByte(0));
    }

    #[test]
    fn parse_int() {
        assert_eq!(parse_value_to_variant("1000", "INT"), Variant::Int16(1000));
        assert_eq!(parse_value_to_variant("-32000", "INT"), Variant::Int16(-32000));
    }

    #[test]
    fn parse_dint() {
        assert_eq!(parse_value_to_variant("100000", "DINT"), Variant::Int32(100000));
    }

    #[test]
    fn parse_lint() {
        assert_eq!(
            parse_value_to_variant("9999999999", "LINT"),
            Variant::Int64(9999999999)
        );
    }

    #[test]
    fn parse_usint() {
        assert_eq!(parse_value_to_variant("255", "USINT"), Variant::Byte(255));
    }

    #[test]
    fn parse_uint() {
        assert_eq!(parse_value_to_variant("65000", "UINT"), Variant::UInt16(65000));
    }

    #[test]
    fn parse_udint() {
        assert_eq!(
            parse_value_to_variant("4000000000", "UDINT"),
            Variant::UInt32(4_000_000_000)
        );
    }

    #[test]
    fn parse_ulint() {
        assert_eq!(
            parse_value_to_variant("18000000000000000000", "ULINT"),
            Variant::UInt64(18_000_000_000_000_000_000)
        );
    }

    // ── Float parsing ──────────────────────────────────────────────

    #[test]
    fn parse_real() {
        let v = parse_value_to_variant("3.140000", "REAL");
        if let Variant::Float(f) = v {
            assert!((f - 3.14).abs() < 0.001);
        } else {
            panic!("Expected Float, got {v:?}");
        }
    }

    #[test]
    fn parse_lreal() {
        let v = parse_value_to_variant("3.141593", "LREAL");
        if let Variant::Double(d) = v {
            assert!((d - 3.141593).abs() < 0.000001);
        } else {
            panic!("Expected Double, got {v:?}");
        }
    }

    #[test]
    fn parse_real_zero() {
        assert_eq!(parse_value_to_variant("0.000000", "REAL"), Variant::Float(0.0));
    }

    // ── String parsing ─────────────────────────────────────────────

    #[test]
    fn parse_string_quoted() {
        let v = parse_value_to_variant("'hello world'", "STRING");
        assert_eq!(v, Variant::String("hello world".to_string().into()));
    }

    #[test]
    fn parse_string_empty() {
        let v = parse_value_to_variant("''", "STRING");
        assert_eq!(v, Variant::String("".to_string().into()));
    }

    #[test]
    fn parse_string_unquoted() {
        let v = parse_value_to_variant("raw", "STRING");
        assert_eq!(v, Variant::String("raw".to_string().into()));
    }

    // ── Time parsing ───────────────────────────────────────────────

    #[test]
    fn parse_time_ms() {
        assert_eq!(parse_time_string("T#500ms"), 500);
        assert_eq!(parse_time_string("t#100ms"), 100);
    }

    #[test]
    fn parse_time_seconds() {
        assert_eq!(parse_time_string("T#1s"), 1000);
        assert_eq!(parse_time_string("T#2s"), 2000);
    }

    #[test]
    fn parse_time_bare_number() {
        assert_eq!(parse_time_string("500"), 500);
        assert_eq!(parse_time_string("0"), 0);
    }

    // ── Round-trip tests (parse → variant → string → parse) ───────

    #[test]
    fn round_trip_bool() {
        let v = parse_value_to_variant("TRUE", "BOOL");
        let s = variant_to_value_string(&v);
        assert_eq!(s, "TRUE");
        let v2 = parse_value_to_variant(&s, "BOOL");
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_int() {
        let v = parse_value_to_variant("42", "INT");
        let s = variant_to_value_string(&v);
        assert_eq!(s, "42");
        let v2 = parse_value_to_variant(&s, "INT");
        assert_eq!(v, v2);
    }

    #[test]
    fn round_trip_real() {
        let v = parse_value_to_variant("3.14", "LREAL");
        let s = variant_to_value_string(&v);
        let v2 = parse_value_to_variant(&s, "LREAL");
        if let (Variant::Double(a), Variant::Double(b)) = (&v, &v2) {
            assert!((a - b).abs() < 0.0001);
        } else {
            panic!("Expected Double variants");
        }
    }

    #[test]
    fn round_trip_string() {
        let v = parse_value_to_variant("'hello'", "STRING");
        let s = variant_to_value_string(&v);
        assert_eq!(s, "hello");
    }

    // ── Invalid input handling ─────────────────────────────────────

    #[test]
    fn parse_invalid_int_defaults_to_zero() {
        assert_eq!(parse_value_to_variant("abc", "INT"), Variant::Int16(0));
    }

    #[test]
    fn parse_invalid_real_defaults_to_zero() {
        assert_eq!(parse_value_to_variant("not_a_number", "REAL"), Variant::Float(0.0));
    }

    // ── BYTE/WORD/DWORD/LWORD aliases ──────────────────────────────

    #[test]
    fn parse_byte_alias() {
        assert_eq!(parse_value_to_variant("200", "BYTE"), Variant::Byte(200));
    }

    #[test]
    fn parse_word_alias() {
        assert_eq!(parse_value_to_variant("1000", "WORD"), Variant::UInt16(1000));
    }

    #[test]
    fn parse_dword_alias() {
        assert_eq!(
            parse_value_to_variant("100000", "DWORD"),
            Variant::UInt32(100000)
        );
    }
}
