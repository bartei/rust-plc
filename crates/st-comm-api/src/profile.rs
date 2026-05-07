//! Device profile: struct schema + register map.
//!
//! A profile defines both the ST struct type (fields visible in code) and the
//! register map (how each field maps to a protocol register on the device).
//! Profiles are YAML files that can be shared across projects.

use serde::{Deserialize, Serialize};

/// A device profile — the bridge between hardware registers and ST code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    /// Struct type name in generated ST code (e.g., "Wago750352").
    pub name: String,

    /// Device vendor/manufacturer.
    #[serde(default)]
    pub vendor: Option<String>,

    /// Primary protocol this profile is designed for.
    #[serde(default)]
    pub protocol: Option<String>,

    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// I/O fields — each becomes a struct member + register mapping.
    pub fields: Vec<ProfileField>,
}

/// A single I/O field in a device profile.
///
/// A field is bound to a transport-specific addressing scheme via
/// **at least one** of:
///
/// - [`ProfileField::register`] — Modbus register address + kind
///   (used by Modbus RTU / TCP / ASCII profiles).
/// - [`ProfileField::upp`] — UPP command mnemonic + decoder
///   (used by Impac / LumaSense pyrometer profiles).
///
/// Both fields are optional in YAML; the runtime layer rejects a
/// profile whose `protocol:` doesn't match any of the bindings the
/// fields actually carry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileField {
    /// Field name in the ST struct (e.g., "DI_0", "SPEED_REF",
    /// "temperature").
    pub name: String,

    /// IEC 61131-3 data type.
    #[serde(rename = "type")]
    pub data_type: FieldDataType,

    /// I/O direction.
    pub direction: FieldDirection,

    /// Modbus register mapping. Required for Modbus profiles, omitted
    /// for non-Modbus protocols (e.g. UPP — see [`Self::upp`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<RegisterMapping>,

    /// UPP (Universal Pyrometer Protocol) command binding. Set on
    /// fields of pyrometer profiles. The runtime resolves the
    /// mnemonic and decoder strings against the `Command` /
    /// `Decoder` enums in the `st-comm-upp` crate at FB
    /// construction time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upp: Option<UppFieldBinding>,

    /// Number of consecutive registers (default 1). When > 1, the field
    /// becomes an array: `ARRAY[0..count-1] OF data_type`.
    #[serde(default = "default_field_count")]
    pub count: u16,

    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Per-field UPP binding parsed from the YAML `upp:` block. The
/// strings are validated against the `Command` and `Decoder` enums
/// in the `st-comm-upp` crate at FB construction time — keeping the
/// validation there means `st-comm-api` does not depend on the UPP
/// crate (which would create a build cycle).
///
/// Spec: see `plan/design_igar.md` "Profile YAML" — the YAML shape is:
///
/// ```yaml
/// fields:
///   - name: temperature
///     type: REAL
///     direction: input
///     upp:
///       command: ms
///       decoder: temp_5d_tenth
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UppFieldBinding {
    /// 2-letter UPP command mnemonic (e.g. `"em"`, `"ms"`, `"ek"`).
    /// Must match a variant of the `Command` enum at FB
    /// construction time.
    pub command: String,

    /// Named decoder for the response payload (e.g.
    /// `"temp_5d_tenth"`, `"u16_dec_milli"`, `"hex_pair_8"`).
    /// Must match a variant of the `Decoder` enum at FB
    /// construction time.
    pub decoder: String,

    /// Optional channel selector for `ek` (1-channel + ratio in one
    /// answer). Accepted values are `"one_channel"` or `"ratio"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

fn default_field_count() -> u16 {
    1
}

/// Supported data types for profile fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FieldDataType {
    Bool,
    Sint,
    Int,
    Dint,
    Lint,
    Usint,
    Uint,
    Udint,
    Ulint,
    Real,
    Lreal,
    Byte,
    Word,
    Dword,
    Lword,
    String,
    Time,
}

impl FieldDataType {
    /// Return the IEC 61131-3 type name for ST code generation.
    pub fn st_type_name(&self) -> &'static str {
        match self {
            Self::Bool => "BOOL",
            Self::Sint => "SINT",
            Self::Int => "INT",
            Self::Dint => "DINT",
            Self::Lint => "LINT",
            Self::Usint => "USINT",
            Self::Uint => "UINT",
            Self::Udint => "UDINT",
            Self::Ulint => "ULINT",
            Self::Real => "REAL",
            Self::Lreal => "LREAL",
            Self::Byte => "BYTE",
            Self::Word => "WORD",
            Self::Dword => "DWORD",
            Self::Lword => "LWORD",
            Self::String => "STRING",
            Self::Time => "TIME",
        }
    }
}

/// I/O direction for a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldDirection {
    /// Device → PLC (read from device).
    Input,
    /// PLC → Device (written to device).
    Output,
    /// Both directions.
    Inout,
}

/// Register mapping: how a field maps to a protocol register.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterMapping {
    /// Register address (Modbus: 0-based register number).
    pub address: u32,

    /// Register type (Modbus-specific, but generalizable).
    pub kind: RegisterKind,

    /// Bit position within the register (for BOOL fields packed into a word).
    #[serde(default)]
    pub bit: Option<u8>,

    /// Scaling factor: ST_value = raw_register_value * scale.
    #[serde(default)]
    pub scale: Option<f64>,

    /// Offset applied after scaling: ST_value = raw * scale + offset.
    #[serde(default)]
    pub offset: Option<f64>,

    /// Engineering unit (documentation only).
    #[serde(default)]
    pub unit: Option<String>,

    /// Byte order for multi-byte registers.
    #[serde(default = "default_byte_order")]
    pub byte_order: ByteOrder,

    /// Number of registers for this field (default 1).
    #[serde(default = "default_word_count")]
    pub word_count: u8,
}

/// Register types (Modbus-centric but extensible).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterKind {
    Coil,
    DiscreteInput,
    HoldingRegister,
    InputRegister,
    /// For simulated devices — no physical register.
    Virtual,
}

/// Byte order for multi-byte register values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ByteOrder {
    BigEndian,
    LittleEndian,
}

fn default_byte_order() -> ByteOrder {
    ByteOrder::BigEndian
}

fn default_word_count() -> u8 {
    1
}

impl DeviceProfile {
    /// Load a profile from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, String> {
        serde_yaml::from_str(yaml).map_err(|e| format!("Invalid device profile YAML: {e}"))
    }

    /// Load a profile from a YAML file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        Self::from_yaml(&content)
    }

    /// Get all input-direction fields.
    pub fn input_fields(&self) -> Vec<&ProfileField> {
        self.fields
            .iter()
            .filter(|f| matches!(f.direction, FieldDirection::Input | FieldDirection::Inout))
            .collect()
    }

    /// Get all output-direction fields.
    pub fn output_fields(&self) -> Vec<&ProfileField> {
        self.fields
            .iter()
            .filter(|f| matches!(f.direction, FieldDirection::Output | FieldDirection::Inout))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_profile() {
        let yaml = r#"
name: TestIO
vendor: TestVendor
protocol: simulated
description: "Test I/O module"
fields:
  - name: DI_0
    type: BOOL
    direction: input
    register: { address: 0, kind: coil }
  - name: DI_1
    type: BOOL
    direction: input
    register: { address: 1, kind: coil }
  - name: AI_0
    type: INT
    direction: input
    register: { address: 0, kind: input_register }
  - name: DO_0
    type: BOOL
    direction: output
    register: { address: 0, kind: coil }
  - name: AO_0
    type: INT
    direction: output
    register: { address: 0, kind: holding_register }
"#;
        let profile = DeviceProfile::from_yaml(yaml).unwrap();
        assert_eq!(profile.name, "TestIO");
        assert_eq!(profile.fields.len(), 5);
        assert_eq!(profile.input_fields().len(), 3);
        assert_eq!(profile.output_fields().len(), 2);
    }

    #[test]
    fn parse_vfd_profile() {
        let yaml = r#"
name: AbbAcs580
vendor: ABB
fields:
  - name: RUN
    type: BOOL
    direction: output
    register: { address: 0, bit: 0, kind: coil }
  - name: SPEED_REF
    type: REAL
    direction: output
    register: { address: 1, kind: holding_register, scale: 0.1, unit: Hz }
  - name: SPEED_ACT
    type: REAL
    direction: input
    register: { address: 2, kind: input_register, scale: 0.1, unit: Hz }
  - name: CURRENT
    type: REAL
    direction: input
    register: { address: 3, kind: input_register, scale: 0.1, unit: A }
"#;
        let profile = DeviceProfile::from_yaml(yaml).unwrap();
        assert_eq!(profile.name, "AbbAcs580");
        assert_eq!(profile.fields.len(), 4);

        let speed_ref = &profile.fields[1];
        assert_eq!(speed_ref.name, "SPEED_REF");
        assert_eq!(speed_ref.data_type, FieldDataType::Real);
        let reg = speed_ref.register.as_ref().expect("Modbus profile carries a register mapping");
        assert_eq!(reg.scale, Some(0.1));
        assert_eq!(reg.unit.as_deref(), Some("Hz"));
    }

    #[test]
    fn field_direction_filtering() {
        let yaml = r#"
name: Mixed
fields:
  - { name: IN1, type: BOOL, direction: input, register: { address: 0, kind: coil } }
  - { name: OUT1, type: BOOL, direction: output, register: { address: 1, kind: coil } }
  - { name: BOTH1, type: INT, direction: inout, register: { address: 0, kind: holding_register } }
"#;
        let profile = DeviceProfile::from_yaml(yaml).unwrap();
        assert_eq!(profile.input_fields().len(), 2); // IN1 + BOTH1
        assert_eq!(profile.output_fields().len(), 2); // OUT1 + BOTH1
    }

    #[test]
    fn register_defaults() {
        let yaml = r#"
name: Defaults
fields:
  - name: X
    type: INT
    direction: input
    register: { address: 5, kind: input_register }
"#;
        let profile = DeviceProfile::from_yaml(yaml).unwrap();
        let reg = profile.fields[0].register.as_ref().expect("present in YAML");
        assert_eq!(reg.byte_order, ByteOrder::BigEndian);
        assert_eq!(reg.word_count, 1);
        assert_eq!(reg.scale, None);
        assert_eq!(reg.offset, None);
    }

    #[test]
    fn upp_field_binding_parses() {
        // A pyrometer profile uses `upp:` instead of `register:` per
        // field. Both are optional in the YAML; the runtime layer
        // routes profiles based on the top-level `protocol:` value.
        let yaml = r#"
name: ImpacIgar6Smart
vendor: Impac
protocol: upp
fields:
  - name: temperature
    type: REAL
    direction: input
    upp:
      command: ms
      decoder: temp_5d_tenth
  - name: ratio_temperature
    type: REAL
    direction: input
    upp:
      command: ek
      decoder: temp_pair_5d_tenth
      channel: ratio
  - name: emissivity
    type: REAL
    direction: inout
    upp:
      command: em
      decoder: u16_dec_milli
"#;
        let profile = DeviceProfile::from_yaml(yaml).unwrap();
        assert_eq!(profile.protocol.as_deref(), Some("upp"));
        assert_eq!(profile.fields.len(), 3);
        // No register mapping on UPP fields.
        for pf in &profile.fields {
            assert!(pf.register.is_none(), "UPP fields must not carry register:");
        }
        // Each field has a UPP binding.
        let temp = &profile.fields[0];
        let upp = temp.upp.as_ref().unwrap();
        assert_eq!(upp.command, "ms");
        assert_eq!(upp.decoder, "temp_5d_tenth");
        assert!(upp.channel.is_none());
        // ek+ratio carries the channel selector.
        let ek = profile.fields[1].upp.as_ref().unwrap();
        assert_eq!(ek.command, "ek");
        assert_eq!(ek.channel.as_deref(), Some("ratio"));
    }

    #[test]
    fn modbus_field_round_trip_keeps_register_some() {
        // Backwards-compat: Modbus profiles still parse cleanly,
        // and `register` is `Some(_)`. This pins the migration story
        // — making `register` optional MUST NOT break existing
        // profiles in profiles/.
        let yaml = r#"
name: TestIO
protocol: modbus-rtu
fields:
  - name: DI_0
    type: BOOL
    direction: input
    register: { address: 0, kind: coil }
"#;
        let profile = DeviceProfile::from_yaml(yaml).unwrap();
        assert_eq!(profile.protocol.as_deref(), Some("modbus-rtu"));
        assert!(profile.fields[0].register.is_some());
        assert!(profile.fields[0].upp.is_none());
    }
}
