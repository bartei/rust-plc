//! Native function blocks: Rust-backed FBs for communication and I/O.
//!
//! Native FBs bridge ST code to actual hardware. They appear as normal
//! `FUNCTION_BLOCK` types in the editor (completions, type checking, debugging)
//! but their `execute()` runs Rust code instead of interpreted ST instructions.

use crate::profile::{DeviceProfile, FieldDataType};
use st_ir::Value;

// ---------------------------------------------------------------------------
// Layout descriptors
// ---------------------------------------------------------------------------

/// Describes which VAR section a native FB field belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFbVarKind {
    /// Configuration parameter set by the caller (VAR_INPUT semantics).
    VarInput,
    /// Internal state field accessible via dot notation (VAR semantics).
    /// Used for all device I/O fields — both readable and writable from ST.
    Var,
}

/// One field in a native FB's layout.
#[derive(Debug, Clone)]
pub struct NativeFbField {
    /// Field name as it appears in ST code (e.g., "DI_0", "slave_id").
    pub name: String,
    /// IEC 61131-3 data type (element type for arrays).
    pub data_type: FieldDataType,
    /// Which VAR section this field belongs to.
    pub var_kind: NativeFbVarKind,
    /// Array dimensions. `None` for scalar fields, `Some([(lo, hi)])` for arrays.
    /// E.g., `Some(vec![(0, 7)])` for `ARRAY[0..7]`.
    pub dimensions: Option<Vec<(i64, i64)>>,
}

/// Complete layout of a native FB type — the single source of truth for
/// the semantic analyzer, compiler, VM, LSP, and DAP.
#[derive(Debug, Clone)]
pub struct NativeFbLayout {
    /// Type name as it appears in ST code (e.g., "Sim8DI4AI4DO2AO").
    pub type_name: String,
    /// All fields in declaration order. The index into this vec matches the
    /// slot index in the compiled `MemoryLayout` and the `execute()` slice.
    pub fields: Vec<NativeFbField>,
}

// ---------------------------------------------------------------------------
// NativeFb trait
// ---------------------------------------------------------------------------

/// A Rust-backed function block.
///
/// Implementors provide the field layout (used at compile time for type checking,
/// completions, and debug info) and an `execute()` method (called at runtime when
/// the FB instance is invoked in a scan cycle).
pub trait NativeFb: Send + Sync {
    /// The type name as it appears in ST code.
    fn type_name(&self) -> &str;

    /// Field layout — single source of truth for all tooling.
    fn layout(&self) -> &NativeFbLayout;

    /// Called when the FB instance is invoked in a scan cycle.
    ///
    /// `fields` is a mutable slice of [`Value`] aligned 1:1 with `layout().fields`.
    /// VAR_INPUT fields have already been applied by the VM before this call.
    /// The implementation should read inputs, perform I/O, and write outputs
    /// back into the slice.
    fn execute(&self, fields: &mut [Value]);
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Central registry of available native FB types.
///
/// Built at startup from device profiles (and eventually plugins). Passed to the
/// semantic analyzer, compiler, and VM so they all see the same set of types.
pub struct NativeFbRegistry {
    entries: Vec<Box<dyn NativeFb>>,
}

impl NativeFbRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a native FB type.
    pub fn register(&mut self, fb: Box<dyn NativeFb>) {
        self.entries.push(fb);
    }

    /// Look up a native FB by type name (case-insensitive).
    pub fn find(&self, type_name: &str) -> Option<&dyn NativeFb> {
        self.entries
            .iter()
            .find(|fb| fb.type_name().eq_ignore_ascii_case(type_name))
            .map(|fb| fb.as_ref())
    }

    /// All registered native FBs.
    pub fn all(&self) -> &[Box<dyn NativeFb>] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for NativeFbRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Profile → NativeFbLayout conversion
// ---------------------------------------------------------------------------

impl DeviceProfile {
    /// Convert this device profile into a [`NativeFbLayout`] suitable for
    /// registering as a native FB type.
    ///
    /// The layout includes:
    /// - `refresh_rate : TIME` as a VarInput configuration parameter
    /// - Diagnostic fields: `connected`, `error_code`, `io_cycles`, `last_response_ms`
    /// - All profile I/O fields as Var (readable + writable from ST code)
    pub fn to_native_fb_layout(&self) -> NativeFbLayout {
        let mut fields = vec![
            // -- VAR_INPUT: configuration parameters --
            NativeFbField {
                name: "refresh_rate".to_string(),
                data_type: FieldDataType::Time,
                var_kind: NativeFbVarKind::VarInput,
                dimensions: None,
            },
            // -- VAR: diagnostic fields --
            NativeFbField {
                name: "connected".to_string(),
                data_type: FieldDataType::Bool,
                var_kind: NativeFbVarKind::Var,
                dimensions: None,
            },
            NativeFbField {
                name: "error_code".to_string(),
                data_type: FieldDataType::Int,
                var_kind: NativeFbVarKind::Var,
                dimensions: None,
            },
            NativeFbField {
                name: "io_cycles".to_string(),
                data_type: FieldDataType::Udint,
                var_kind: NativeFbVarKind::Var,
                dimensions: None,
            },
            NativeFbField {
                name: "last_response_ms".to_string(),
                data_type: FieldDataType::Real,
                var_kind: NativeFbVarKind::Var,
                dimensions: None,
            },
        ];

        // -- VAR: I/O fields from the profile --
        for pf in &self.fields {
            let dims = if pf.count > 1 {
                Some(vec![(0, pf.count as i64 - 1)])
            } else {
                None
            };
            fields.push(NativeFbField {
                name: pf.name.clone(),
                data_type: pf.data_type,
                var_kind: NativeFbVarKind::Var,
                dimensions: dims,
            });
        }

        NativeFbLayout {
            type_name: self.name.clone(),
            fields,
        }
    }

    /// Build a [`NativeFbLayout`] for a Modbus RTU device profile.
    ///
    /// The layout follows the two-layer model: the device takes a `link`
    /// parameter (the serial port path from a SerialLink instance) instead of
    /// owning serial config fields. This separates transport (link) from
    /// protocol (device).
    ///
    /// Layout: link, slave_id, refresh_rate, diagnostics, profile I/O fields.
    pub fn to_modbus_rtu_device_layout(&self) -> NativeFbLayout {
        let mut fields = vec![
            // -- VAR_INPUT: link binding + protocol parameters --
            NativeFbField { name: "link".into(), data_type: FieldDataType::String, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            NativeFbField { name: "slave_id".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            NativeFbField { name: "refresh_rate".into(), data_type: FieldDataType::Time, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            // -- VAR: diagnostic fields --
            NativeFbField { name: "connected".into(), data_type: FieldDataType::Bool, var_kind: NativeFbVarKind::Var, dimensions: None },
            NativeFbField { name: "error_code".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::Var, dimensions: None },
            NativeFbField { name: "io_cycles".into(), data_type: FieldDataType::Udint, var_kind: NativeFbVarKind::Var, dimensions: None },
            NativeFbField { name: "last_response_ms".into(), data_type: FieldDataType::Real, var_kind: NativeFbVarKind::Var, dimensions: None },
        ];

        // -- VAR: I/O fields from the profile --
        for pf in &self.fields {
            let dims = if pf.count > 1 {
                Some(vec![(0, pf.count as i64 - 1)])
            } else {
                None
            };
            fields.push(NativeFbField {
                name: pf.name.clone(),
                data_type: pf.data_type,
                var_kind: NativeFbVarKind::Var,
                dimensions: dims,
            });
        }

        NativeFbLayout {
            type_name: self.name.clone(),
            fields,
        }
    }

    /// Build a [`NativeFbLayout`] for a Modbus TCP device profile.
    ///
    /// Unlike RTU (which takes a `link` reference to a separate SerialLink),
    /// TCP devices own their connection directly: `host` + `port` + `unit_id`.
    ///
    /// Layout: host, port, unit_id, refresh_rate, diagnostics, profile I/O fields.
    pub fn to_modbus_tcp_device_layout(&self) -> NativeFbLayout {
        let mut fields = vec![
            // -- VAR_INPUT: connection + protocol parameters --
            NativeFbField { name: "host".into(), data_type: FieldDataType::String, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            NativeFbField { name: "port".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            NativeFbField { name: "unit_id".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            NativeFbField { name: "refresh_rate".into(), data_type: FieldDataType::Time, var_kind: NativeFbVarKind::VarInput, dimensions: None },
            // -- VAR: diagnostic fields --
            NativeFbField { name: "connected".into(), data_type: FieldDataType::Bool, var_kind: NativeFbVarKind::Var, dimensions: None },
            NativeFbField { name: "error_code".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::Var, dimensions: None },
            NativeFbField { name: "io_cycles".into(), data_type: FieldDataType::Udint, var_kind: NativeFbVarKind::Var, dimensions: None },
            NativeFbField { name: "last_response_ms".into(), data_type: FieldDataType::Real, var_kind: NativeFbVarKind::Var, dimensions: None },
        ];

        // -- VAR: I/O fields from the profile --
        for pf in &self.fields {
            let dims = if pf.count > 1 {
                Some(vec![(0, pf.count as i64 - 1)])
            } else {
                None
            };
            fields.push(NativeFbField {
                name: pf.name.clone(),
                data_type: pf.data_type,
                var_kind: NativeFbVarKind::Var,
                dimensions: dims,
            });
        }

        NativeFbLayout {
            type_name: self.name.clone(),
            fields,
        }
    }
}

/// Map a [`FieldDataType`] to the corresponding [`st_ir::VarType`].
pub fn field_data_type_to_var_type(dt: FieldDataType) -> st_ir::VarType {
    match dt {
        FieldDataType::Bool => st_ir::VarType::Bool,
        FieldDataType::Sint | FieldDataType::Int | FieldDataType::Dint | FieldDataType::Lint => {
            st_ir::VarType::Int
        }
        FieldDataType::Usint
        | FieldDataType::Uint
        | FieldDataType::Udint
        | FieldDataType::Ulint
        | FieldDataType::Byte
        | FieldDataType::Word
        | FieldDataType::Dword
        | FieldDataType::Lword => st_ir::VarType::UInt,
        FieldDataType::Real | FieldDataType::Lreal => st_ir::VarType::Real,
        FieldDataType::String => st_ir::VarType::String,
        FieldDataType::Time => st_ir::VarType::Time,
    }
}

/// Map a [`FieldDataType`] to the corresponding [`st_ir::IntWidth`].
pub fn field_data_type_to_int_width(dt: FieldDataType) -> st_ir::IntWidth {
    match dt {
        FieldDataType::Sint => st_ir::IntWidth::I8,
        FieldDataType::Usint | FieldDataType::Byte => st_ir::IntWidth::U8,
        FieldDataType::Int => st_ir::IntWidth::I16,
        FieldDataType::Uint | FieldDataType::Word => st_ir::IntWidth::U16,
        FieldDataType::Dint => st_ir::IntWidth::I32,
        FieldDataType::Udint | FieldDataType::Dword => st_ir::IntWidth::U32,
        FieldDataType::Lint => st_ir::IntWidth::I64,
        FieldDataType::Ulint | FieldDataType::Lword => st_ir::IntWidth::U64,
        _ => st_ir::IntWidth::None,
    }
}

/// Convert a [`NativeFbLayout`] into a compiled [`st_ir::MemoryLayout`].
///
/// For scalar fields, offset increments by 1 (one `Value` slot).
/// For array fields, offset increments by the element count (elements stored
/// inline in the FB's `Vec<Value>`).
///
/// `type_defs` is appended with `TypeDef::Array` entries for array fields.
/// `type_def_base` is the current length of the caller's type_defs vec
/// (so `VarType::Array(idx)` references are correct).
pub fn layout_to_memory_layout(
    layout: &NativeFbLayout,
    type_defs: &mut Vec<st_ir::TypeDef>,
    type_def_base: u16,
) -> st_ir::MemoryLayout {
    let mut offset = 0;
    let slots = layout
        .fields
        .iter()
        .map(|f| {
            let elem_ty = field_data_type_to_var_type(f.data_type);
            let int_width = field_data_type_to_int_width(f.data_type);

            if let Some(dims) = &f.dimensions {
                // Array field: create TypeDef, use expanded size
                let td_idx = type_def_base + type_defs.len() as u16;
                type_defs.push(st_ir::TypeDef::Array {
                    element_type: elem_ty,
                    dimensions: dims.clone(),
                });
                let count: usize = dims.iter().map(|(lo, hi)| (hi - lo + 1) as usize).product();
                let slot = st_ir::VarSlot {
                    name: f.name.clone(),
                    ty: st_ir::VarType::Array(td_idx),
                    offset,
                    size: count,
                    retain: false,
                    persistent: false,
                    int_width,
                };
                offset += count;
                slot
            } else {
                // Scalar field: 1 Value slot regardless of byte size
                let slot = st_ir::VarSlot {
                    name: f.name.clone(),
                    ty: elem_ty,
                    offset,
                    size: 1,
                    retain: false,
                    persistent: false,
                    int_width,
                };
                offset += 1;
                slot
            }
        })
        .collect();
    st_ir::MemoryLayout { slots }
}

/// Compute the expanded size of a native FB's field slice (total `Value` count).
///
/// For scalar fields: 1. For array fields: element count. This is the size
/// of the `&mut [Value]` slice that `execute()` receives.
pub fn expanded_field_count(layout: &NativeFbLayout) -> usize {
    layout.fields.iter().map(|f| {
        if let Some(dims) = &f.dimensions {
            dims.iter().map(|(lo, hi)| (hi - lo + 1) as usize).product()
        } else {
            1
        }
    }).sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial native FB for testing.
    struct CounterFb {
        layout: NativeFbLayout,
    }

    impl CounterFb {
        fn new() -> Self {
            Self {
                layout: NativeFbLayout {
                    type_name: "Counter".to_string(),
                    fields: vec![NativeFbField {
                        name: "count".to_string(),
                        data_type: FieldDataType::Int,
                        var_kind: NativeFbVarKind::Var,
                        dimensions: None,
                    }],
                },
            }
        }
    }

    impl NativeFb for CounterFb {
        fn type_name(&self) -> &str {
            &self.layout.type_name
        }
        fn layout(&self) -> &NativeFbLayout {
            &self.layout
        }
        fn execute(&self, fields: &mut [Value]) {
            let count = fields[0].as_int();
            fields[0] = Value::Int(count + 1);
        }
    }

    #[test]
    fn registry_register_and_find() {
        let mut reg = NativeFbRegistry::new();
        assert!(reg.is_empty());

        reg.register(Box::new(CounterFb::new()));
        assert_eq!(reg.len(), 1);
        assert!(!reg.is_empty());

        // Case-insensitive lookup
        assert!(reg.find("Counter").is_some());
        assert!(reg.find("counter").is_some());
        assert!(reg.find("COUNTER").is_some());
        assert!(reg.find("nonexistent").is_none());
    }

    #[test]
    fn registry_all() {
        let mut reg = NativeFbRegistry::new();
        reg.register(Box::new(CounterFb::new()));
        assert_eq!(reg.all().len(), 1);
        assert_eq!(reg.all()[0].type_name(), "Counter");
    }

    #[test]
    fn counter_fb_execute() {
        let fb = CounterFb::new();
        let mut fields = vec![Value::Int(0)];
        fb.execute(&mut fields);
        assert_eq!(fields[0].as_int(), 1);
        fb.execute(&mut fields);
        assert_eq!(fields[0].as_int(), 2);
    }

    #[test]
    fn profile_to_layout() {
        let profile = DeviceProfile::from_yaml(
            r#"
name: TestIO
protocol: simulated
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: virtual } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 1, kind: virtual } }
  - { name: AI_0, type: INT, direction: input, register: { address: 2, kind: virtual } }
"#,
        )
        .unwrap();

        let layout = profile.to_native_fb_layout();
        assert_eq!(layout.type_name, "TestIO");

        // Expected: refresh_rate + 4 diag fields + 3 profile fields = 8
        assert_eq!(layout.fields.len(), 8);

        // First field is refresh_rate (VarInput)
        assert_eq!(layout.fields[0].name, "refresh_rate");
        assert_eq!(layout.fields[0].var_kind, NativeFbVarKind::VarInput);

        // Diagnostic fields
        assert_eq!(layout.fields[1].name, "connected");
        assert_eq!(layout.fields[2].name, "error_code");
        assert_eq!(layout.fields[3].name, "io_cycles");
        assert_eq!(layout.fields[4].name, "last_response_ms");

        // Profile fields (all Var)
        assert_eq!(layout.fields[5].name, "DI_0");
        assert_eq!(layout.fields[5].var_kind, NativeFbVarKind::Var);
        assert_eq!(layout.fields[6].name, "DO_0");
        assert_eq!(layout.fields[7].name, "AI_0");
    }

    #[test]
    fn layout_to_memory_layout_roundtrip() {
        let layout = NativeFbLayout {
            type_name: "Test".to_string(),
            fields: vec![
                NativeFbField {
                    name: "flag".to_string(),
                    data_type: FieldDataType::Bool,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
                NativeFbField {
                    name: "count".to_string(),
                    data_type: FieldDataType::Int,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
                NativeFbField {
                    name: "value".to_string(),
                    data_type: FieldDataType::Real,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
            ],
        };

        let mut td = Vec::new();
        let mem = layout_to_memory_layout(&layout, &mut td, 0);
        assert_eq!(mem.slots.len(), 3);

        assert_eq!(mem.slots[0].name, "flag");
        assert_eq!(mem.slots[0].ty, st_ir::VarType::Bool);

        assert_eq!(mem.slots[1].name, "count");
        assert_eq!(mem.slots[1].ty, st_ir::VarType::Int);
        assert_eq!(mem.slots[1].int_width, st_ir::IntWidth::I16);

        assert_eq!(mem.slots[2].name, "value");
        assert_eq!(mem.slots[2].ty, st_ir::VarType::Real);
    }

    #[test]
    fn field_data_type_mappings() {
        assert_eq!(field_data_type_to_var_type(FieldDataType::Bool), st_ir::VarType::Bool);
        assert_eq!(field_data_type_to_var_type(FieldDataType::Sint), st_ir::VarType::Int);
        assert_eq!(field_data_type_to_var_type(FieldDataType::Uint), st_ir::VarType::UInt);
        assert_eq!(field_data_type_to_var_type(FieldDataType::Real), st_ir::VarType::Real);
        assert_eq!(field_data_type_to_var_type(FieldDataType::Time), st_ir::VarType::Time);
        assert_eq!(field_data_type_to_var_type(FieldDataType::String), st_ir::VarType::String);

        assert_eq!(field_data_type_to_int_width(FieldDataType::Sint), st_ir::IntWidth::I8);
        assert_eq!(field_data_type_to_int_width(FieldDataType::Usint), st_ir::IntWidth::U8);
        assert_eq!(field_data_type_to_int_width(FieldDataType::Int), st_ir::IntWidth::I16);
        assert_eq!(field_data_type_to_int_width(FieldDataType::Dint), st_ir::IntWidth::I32);
        assert_eq!(field_data_type_to_int_width(FieldDataType::Lint), st_ir::IntWidth::I64);
        assert_eq!(field_data_type_to_int_width(FieldDataType::Bool), st_ir::IntWidth::None);
        assert_eq!(field_data_type_to_int_width(FieldDataType::Real), st_ir::IntWidth::None);
    }
}
