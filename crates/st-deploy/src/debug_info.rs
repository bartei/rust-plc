//! Debug information extraction, stripping, and obfuscation.
//!
//! The compiled `st_ir::Module` embeds debug-relevant data inline:
//! - `Function.name` — POU names
//! - `Function.source_map` — instruction → source byte offsets
//! - `VarSlot.name` — variable names in locals and globals
//! - `TypeDef` names and field names
//!
//! For **development** bundles, all this data stays in the Module and is also
//! extracted into a standalone `DebugMap` for the debugger.
//!
//! For **release** bundles, all names and source maps are stripped from the
//! Module before serialization. No `DebugMap` is included.
//!
//! For **release-debug** bundles, variable names are replaced with opaque
//! indices (`v0`, `v1`, …) and source maps are moved out of the Module into
//! an obfuscated `DebugMap` that has line maps but no original variable names.

use serde::{Deserialize, Serialize};
use st_ir::{MemoryLayout, Module, SourceLocation, TypeDef};

/// Standalone debug information extracted from a compiled Module.
///
/// Stored as `debug.map` (JSON) inside the bundle archive. The runtime/agent
/// can use this for debugger features (breakpoints, stack traces, variable
/// display) without the debug info being embedded in the bytecode itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugMap {
    /// Per-function debug info, indexed by function index in the Module.
    pub functions: Vec<FunctionDebugInfo>,
    /// Global variable names, indexed by slot index.
    pub global_names: Vec<String>,
    /// Type definition names (structs, enums).
    pub type_names: Vec<String>,
}

/// Debug info for a single compiled function/FB/program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDebugInfo {
    /// Original POU name.
    pub name: String,
    /// Local variable names, indexed by slot index.
    pub local_names: Vec<String>,
    /// Source map: instruction index → source byte offsets.
    pub source_map: Vec<SourceLocation>,
}

/// Extract a full `DebugMap` from a compiled Module.
///
/// This captures all debug-relevant data before any stripping occurs.
pub fn extract_debug_map(module: &Module) -> DebugMap {
    let functions = module
        .functions
        .iter()
        .map(|f| FunctionDebugInfo {
            name: f.name.clone(),
            local_names: f.locals.slots.iter().map(|s| s.name.clone()).collect(),
            source_map: f.source_map.clone(),
        })
        .collect();

    let global_names = module
        .globals
        .slots
        .iter()
        .map(|s| s.name.clone())
        .collect();

    let type_names = module
        .type_defs
        .iter()
        .map(|td| match td {
            TypeDef::Struct { name, .. } => name.clone(),
            TypeDef::Enum { name, .. } => name.clone(),
            TypeDef::Array { .. } => String::new(),
        })
        .collect();

    DebugMap {
        functions,
        global_names,
        type_names,
    }
}

/// Create an obfuscated `DebugMap` for release-debug mode.
///
/// Keeps source maps (for stack traces with line numbers) but replaces all
/// variable names with opaque indices (`v0`, `v1`, …). POU names are kept
/// so stack traces show which function crashed.
pub fn obfuscate_debug_map(debug_map: &DebugMap) -> DebugMap {
    let functions = debug_map
        .functions
        .iter()
        .map(|f| FunctionDebugInfo {
            name: f.name.clone(), // keep POU names for stack traces
            local_names: (0..f.local_names.len())
                .map(|i| format!("v{i}"))
                .collect(),
            source_map: f.source_map.clone(), // keep line maps
        })
        .collect();

    let global_names = (0..debug_map.global_names.len())
        .map(|i| format!("g{i}"))
        .collect();

    let type_names = (0..debug_map.type_names.len())
        .map(|i| format!("t{i}"))
        .collect();

    DebugMap {
        functions,
        global_names,
        type_names,
    }
}

/// Strip all debug information from a Module for release mode.
///
/// Removes: variable names (replaced with indices), source maps (cleared),
/// type definition names (replaced with indices). The resulting Module is
/// functionally identical but contains no human-readable identifiers from
/// the original source code.
///
/// **Important:** POU names (`Function.name`) are kept because the runtime
/// needs them to locate the entry point PROGRAM by name. The `find_function`
/// method relies on name matching. Use `obfuscate_function_names` separately
/// if POU name obfuscation is desired.
pub fn strip_module(module: &mut Module) {
    // Strip global variable names
    strip_memory_layout(&mut module.globals, "g");

    // Strip per-function data
    for (i, func) in module.functions.iter_mut().enumerate() {
        // Strip local variable names
        strip_memory_layout(&mut func.locals, &format!("f{i}_v"));

        // Clear source maps
        func.source_map.clear();
    }

    // Strip type definition names and field names
    for (i, td) in module.type_defs.iter_mut().enumerate() {
        match td {
            TypeDef::Struct { name, fields } => {
                *name = format!("t{i}");
                for (j, field) in fields.iter_mut().enumerate() {
                    field.name = format!("f{j}");
                }
            }
            TypeDef::Enum { name, variants } => {
                *name = format!("t{i}");
                for (j, (vname, _)) in variants.iter_mut().enumerate() {
                    *vname = format!("e{j}");
                }
            }
            TypeDef::Array { .. } => {}
        }
    }
}

/// Strip a Module for release-debug mode.
///
/// Same as `strip_module` but keeps source maps in the Module so the runtime
/// can report instruction → byte offset mappings. Variable names are still
/// replaced with opaque indices.
pub fn strip_module_keep_source_maps(module: &mut Module) {
    // Strip global variable names
    strip_memory_layout(&mut module.globals, "g");

    // Strip per-function data — but keep source maps
    for (i, func) in module.functions.iter_mut().enumerate() {
        strip_memory_layout(&mut func.locals, &format!("f{i}_v"));
        // source_map is NOT cleared — kept for line-based breakpoints
    }

    // Strip type definition names and field names
    for (i, td) in module.type_defs.iter_mut().enumerate() {
        match td {
            TypeDef::Struct { name, fields } => {
                *name = format!("t{i}");
                for (j, field) in fields.iter_mut().enumerate() {
                    field.name = format!("f{j}");
                }
            }
            TypeDef::Enum { name, variants } => {
                *name = format!("t{i}");
                for (j, (vname, _)) in variants.iter_mut().enumerate() {
                    *vname = format!("e{j}");
                }
            }
            TypeDef::Array { .. } => {}
        }
    }
}

fn strip_memory_layout(layout: &mut MemoryLayout, prefix: &str) {
    for (i, slot) in layout.slots.iter_mut().enumerate() {
        slot.name = format!("{prefix}{i}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use st_ir::*;

    fn sample_module() -> Module {
        Module {
            functions: vec![
                Function {
                    name: "Main".to_string(),
                    kind: PouKind::Program,
                    register_count: 4,
                    instructions: vec![Instruction::Nop],
                    label_positions: vec![],
                    locals: MemoryLayout {
                        slots: vec![
                            VarSlot {
                                name: "counter".to_string(),
                                ty: VarType::Int,
                                offset: 0,
                                size: 8,
                                retain: false,
                                persistent: false,
                                int_width: IntWidth::I16,
                            },
                            VarSlot {
                                name: "motor_speed".to_string(),
                                ty: VarType::Real,
                                offset: 8,
                                size: 8,
                                retain: false,
                                persistent: false,
                                int_width: IntWidth::None,
                            },
                        ],
                    },
                    source_map: vec![
                        SourceLocation {
                            byte_offset: 10,
                            byte_end: 25,
                        },
                    ],
                    body_start_pc: 0,
                },
                Function {
                    name: "Helper".to_string(),
                    kind: PouKind::Function,
                    register_count: 2,
                    instructions: vec![Instruction::Nop],
                    label_positions: vec![],
                    locals: MemoryLayout {
                        slots: vec![VarSlot {
                            name: "input_val".to_string(),
                            ty: VarType::Int,
                            offset: 0,
                            size: 8,
                            retain: false,
                            persistent: false,
                            int_width: IntWidth::None,
                        }],
                    },
                    source_map: vec![
                        SourceLocation {
                            byte_offset: 100,
                            byte_end: 120,
                        },
                    ],
                    body_start_pc: 0,
                },
            ],
            globals: MemoryLayout {
                slots: vec![
                    VarSlot {
                        name: "alarm_active".to_string(),
                        ty: VarType::Bool,
                        offset: 0,
                        size: 1,
                        retain: false,
                        persistent: false,
                        int_width: IntWidth::None,
                    },
                    VarSlot {
                        name: "tank_level".to_string(),
                        ty: VarType::Real,
                        offset: 1,
                        size: 8,
                        retain: false,
                        persistent: false,
                        int_width: IntWidth::None,
                    },
                ],
            },
            type_defs: vec![TypeDef::Struct {
                name: "MotorState".to_string(),
                fields: vec![VarSlot {
                    name: "running".to_string(),
                    ty: VarType::Bool,
                    offset: 0,
                    size: 1,
                    retain: false,
                    persistent: false,
                    int_width: IntWidth::None,
                }],
            }],
            native_fb_indices: vec![],
        }
    }

    #[test]
    fn extract_debug_map_captures_all_names() {
        let module = sample_module();
        let dm = extract_debug_map(&module);

        assert_eq!(dm.functions.len(), 2);
        assert_eq!(dm.functions[0].name, "Main");
        assert_eq!(dm.functions[0].local_names, vec!["counter", "motor_speed"]);
        assert_eq!(dm.functions[0].source_map.len(), 1);
        assert_eq!(dm.functions[1].name, "Helper");
        assert_eq!(dm.functions[1].local_names, vec!["input_val"]);

        assert_eq!(dm.global_names, vec!["alarm_active", "tank_level"]);
        assert_eq!(dm.type_names, vec!["MotorState"]);
    }

    #[test]
    fn obfuscate_replaces_var_names_keeps_pou_names() {
        let module = sample_module();
        let dm = extract_debug_map(&module);
        let obf = obfuscate_debug_map(&dm);

        // POU names kept
        assert_eq!(obf.functions[0].name, "Main");
        assert_eq!(obf.functions[1].name, "Helper");

        // Variable names replaced
        assert_eq!(obf.functions[0].local_names, vec!["v0", "v1"]);
        assert_eq!(obf.functions[1].local_names, vec!["v0"]);
        assert_eq!(obf.global_names, vec!["g0", "g1"]);
        assert_eq!(obf.type_names, vec!["t0"]);

        // Source maps preserved
        assert_eq!(obf.functions[0].source_map.len(), 1);
        assert_eq!(obf.functions[0].source_map[0].byte_offset, 10);
    }

    #[test]
    fn strip_module_removes_names_and_source_maps() {
        let mut module = sample_module();
        strip_module(&mut module);

        // Variable names replaced with indices
        assert_eq!(module.globals.slots[0].name, "g0");
        assert_eq!(module.globals.slots[1].name, "g1");
        assert_eq!(module.functions[0].locals.slots[0].name, "f0_v0");
        assert_eq!(module.functions[0].locals.slots[1].name, "f0_v1");
        assert_eq!(module.functions[1].locals.slots[0].name, "f1_v0");

        // Source maps cleared
        assert!(module.functions[0].source_map.is_empty());
        assert!(module.functions[1].source_map.is_empty());

        // POU names kept (runtime needs them)
        assert_eq!(module.functions[0].name, "Main");
        assert_eq!(module.functions[1].name, "Helper");

        // Type def names stripped
        match &module.type_defs[0] {
            TypeDef::Struct { name, fields } => {
                assert_eq!(name, "t0");
                assert_eq!(fields[0].name, "f0");
            }
            _ => panic!("expected struct"),
        }

        // Module still functions — instructions and types intact
        assert_eq!(module.functions[0].instructions.len(), 1);
        assert_eq!(module.functions[0].register_count, 4);
    }

    #[test]
    fn strip_module_keep_source_maps_preserves_maps() {
        let mut module = sample_module();
        strip_module_keep_source_maps(&mut module);

        // Names stripped
        assert_eq!(module.globals.slots[0].name, "g0");
        assert_eq!(module.functions[0].locals.slots[0].name, "f0_v0");

        // Source maps kept
        assert_eq!(module.functions[0].source_map.len(), 1);
        assert_eq!(module.functions[0].source_map[0].byte_offset, 10);
        assert_eq!(module.functions[1].source_map.len(), 1);
    }

    #[test]
    fn original_names_not_in_stripped_json() {
        let mut module = sample_module();
        strip_module(&mut module);

        let json = serde_json::to_string(&module).unwrap();

        assert!(!json.contains("counter"), "stripped JSON should not contain 'counter'");
        assert!(!json.contains("motor_speed"), "stripped JSON should not contain 'motor_speed'");
        assert!(!json.contains("alarm_active"), "stripped JSON should not contain 'alarm_active'");
        assert!(!json.contains("tank_level"), "stripped JSON should not contain 'tank_level'");
        assert!(!json.contains("input_val"), "stripped JSON should not contain 'input_val'");
        assert!(!json.contains("MotorState"), "stripped JSON should not contain 'MotorState'");
        assert!(!json.contains("running"), "stripped JSON should not contain 'running'");

        // POU names are still there (needed by runtime)
        assert!(json.contains("Main"));
        assert!(json.contains("Helper"));
    }

    #[test]
    fn debug_map_roundtrip_json() {
        let module = sample_module();
        let dm = extract_debug_map(&module);

        let json = serde_json::to_string_pretty(&dm).unwrap();
        let parsed: DebugMap = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.functions.len(), 2);
        assert_eq!(parsed.functions[0].name, "Main");
        assert_eq!(parsed.global_names, vec!["alarm_active", "tank_level"]);
    }

    #[test]
    fn obfuscated_debug_map_has_no_original_names() {
        let module = sample_module();
        let dm = extract_debug_map(&module);
        let obf = obfuscate_debug_map(&dm);

        let json = serde_json::to_string(&obf).unwrap();

        assert!(!json.contains("counter"));
        assert!(!json.contains("motor_speed"));
        assert!(!json.contains("alarm_active"));
        assert!(!json.contains("tank_level"));
        assert!(!json.contains("input_val"));
        assert!(!json.contains("MotorState"));

        // POU names and source offsets are present
        assert!(json.contains("Main"));
        assert!(json.contains("Helper"));
        assert!(json.contains("byte_offset"));
    }
}
