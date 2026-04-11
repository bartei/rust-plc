//! Online Change Manager: hot-reload PLC programs without losing state.
//!
//! Compares old and new compiled modules, determines compatibility,
//! migrates variable state, and atomically swaps the running program.

use st_ir::*;
use std::collections::HashMap;

/// Result of analyzing compatibility between old and new modules.
#[derive(Debug, Clone)]
pub struct ChangeAnalysis {
    /// Whether the change is compatible (can be applied without restart).
    pub compatible: bool,
    /// Per-function change details.
    pub function_changes: Vec<FunctionChange>,
    /// Reasons why the change is incompatible (empty if compatible).
    pub incompatible_reasons: Vec<String>,
    /// Variables that will be preserved.
    pub preserved_vars: Vec<String>,
    /// Variables that are new and will be initialized to defaults.
    pub new_vars: Vec<String>,
    /// Variables that were removed.
    pub removed_vars: Vec<String>,
}

/// Change details for a single function/POU.
#[derive(Debug, Clone)]
pub struct FunctionChange {
    pub name: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// No changes to this function.
    Unchanged,
    /// Code changed but variable layout is the same — compatible.
    CodeOnly,
    /// New variables added but existing ones unchanged — compatible.
    VarsAdded,
    /// Variable types or order changed — incompatible.
    LayoutChanged,
    /// Function is new (didn't exist before).
    New,
    /// Function was removed.
    Removed,
}

/// Analyze compatibility between an old and new module.
pub fn analyze_change(old: &Module, new: &Module) -> ChangeAnalysis {
    let mut function_changes = Vec::new();
    let mut incompatible_reasons = Vec::new();
    let mut preserved_vars = Vec::new();
    let mut new_vars = Vec::new();
    let mut removed_vars = Vec::new();

    let old_funcs: HashMap<String, &Function> = old
        .functions
        .iter()
        .map(|f| (f.name.to_uppercase(), f))
        .collect();
    let new_funcs: HashMap<String, &Function> = new
        .functions
        .iter()
        .map(|f| (f.name.to_uppercase(), f))
        .collect();

    // Check each function in the new module
    for new_func in &new.functions {
        let key = new_func.name.to_uppercase();
        if let Some(old_func) = old_funcs.get(&key) {
            let change = compare_functions(old_func, new_func);

            // Track variable changes for programs/FBs
            if new_func.kind == PouKind::Program || new_func.kind == PouKind::FunctionBlock {
                for new_slot in &new_func.locals.slots {
                    if old_func
                        .locals
                        .slots
                        .iter()
                        .any(|s| s.name.eq_ignore_ascii_case(&new_slot.name) && s.ty == new_slot.ty)
                    {
                        preserved_vars.push(format!("{}.{}", new_func.name, new_slot.name));
                    } else if old_func
                        .locals
                        .slots
                        .iter()
                        .any(|s| s.name.eq_ignore_ascii_case(&new_slot.name))
                    {
                        // Same name, different type — incompatible
                        incompatible_reasons.push(format!(
                            "Variable '{}.{}' changed type",
                            new_func.name, new_slot.name
                        ));
                    } else {
                        new_vars.push(format!("{}.{}", new_func.name, new_slot.name));
                    }
                }

                for old_slot in &old_func.locals.slots {
                    if !new_func
                        .locals
                        .slots
                        .iter()
                        .any(|s| s.name.eq_ignore_ascii_case(&old_slot.name))
                    {
                        removed_vars.push(format!("{}.{}", old_func.name, old_slot.name));
                    }
                }
            }

            if change == ChangeKind::LayoutChanged {
                incompatible_reasons.push(format!(
                    "Function '{}' has incompatible variable layout changes",
                    new_func.name
                ));
            }

            function_changes.push(FunctionChange {
                name: new_func.name.clone(),
                kind: change,
            });
        } else {
            function_changes.push(FunctionChange {
                name: new_func.name.clone(),
                kind: ChangeKind::New,
            });
        }
    }

    // Check for removed functions
    for old_func in &old.functions {
        let key = old_func.name.to_uppercase();
        if !new_funcs.contains_key(&key) {
            function_changes.push(FunctionChange {
                name: old_func.name.clone(),
                kind: ChangeKind::Removed,
            });
            incompatible_reasons.push(format!("Function '{}' was removed", old_func.name));
        }
    }

    // Check global variable compatibility
    check_globals_compat(old, new, &mut incompatible_reasons, &mut preserved_vars, &mut new_vars, &mut removed_vars);

    ChangeAnalysis {
        compatible: incompatible_reasons.is_empty(),
        function_changes,
        incompatible_reasons,
        preserved_vars,
        new_vars,
        removed_vars,
    }
}

/// Compare two versions of the same function.
fn compare_functions(old: &Function, new: &Function) -> ChangeKind {
    // Check if variable layout changed
    let layout_same = old.locals.slots.len() <= new.locals.slots.len()
        && old.locals.slots.iter().enumerate().all(|(i, old_slot)| {
            new.locals
                .slots
                .get(i)
                .map(|new_slot| {
                    old_slot.name.eq_ignore_ascii_case(&new_slot.name) && old_slot.ty == new_slot.ty
                })
                .unwrap_or(false)
        });

    if !layout_same {
        // Check if only new vars were added at the end
        let all_old_preserved = old.locals.slots.iter().all(|old_slot| {
            new.locals
                .slots
                .iter()
                .any(|new_slot| {
                    old_slot.name.eq_ignore_ascii_case(&new_slot.name) && old_slot.ty == new_slot.ty
                })
        });

        if all_old_preserved && new.locals.slots.len() > old.locals.slots.len() {
            return ChangeKind::VarsAdded;
        }

        return ChangeKind::LayoutChanged;
    }

    // Layout is the same — check if code changed
    if old.instructions.len() != new.instructions.len() {
        return ChangeKind::CodeOnly;
    }

    // Same instruction count — check if instructions differ
    // (simplified: compare instruction count as proxy)
    if new.locals.slots.len() > old.locals.slots.len() {
        ChangeKind::VarsAdded
    } else {
        ChangeKind::CodeOnly
    }
}

fn check_globals_compat(
    old: &Module,
    new: &Module,
    reasons: &mut Vec<String>,
    preserved: &mut Vec<String>,
    new_vars: &mut Vec<String>,
    removed: &mut Vec<String>,
) {
    for new_slot in &new.globals.slots {
        if let Some(old_slot) = old
            .globals
            .slots
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&new_slot.name))
        {
            if old_slot.ty != new_slot.ty {
                reasons.push(format!(
                    "Global variable '{}' changed type",
                    new_slot.name
                ));
            } else {
                preserved.push(format!("GLOBAL.{}", new_slot.name));
            }
        } else {
            new_vars.push(format!("GLOBAL.{}", new_slot.name));
        }
    }

    for old_slot in &old.globals.slots {
        if !new
            .globals
            .slots
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(&old_slot.name))
        {
            removed.push(format!("GLOBAL.{}", old_slot.name));
        }
    }
}

/// Migrate variable state from old locals to new locals layout.
/// Returns the new locals vector with preserved values and defaults for new vars.
pub fn migrate_locals(
    old_locals: &[Value],
    old_layout: &MemoryLayout,
    new_layout: &MemoryLayout,
) -> Vec<Value> {
    new_layout
        .slots
        .iter()
        .map(|new_slot| {
            // Try to find the same variable in old layout
            if let Some((old_idx, old_slot)) = old_layout.find_slot(&new_slot.name) {
                if old_slot.ty == new_slot.ty {
                    // Same type — preserve value
                    return old_locals
                        .get(old_idx as usize)
                        .cloned()
                        .unwrap_or_else(|| Value::default_for_type(new_slot.ty));
                }
            }
            // New variable or type changed — use default
            Value::default_for_type(new_slot.ty)
        })
        .collect()
}

/// Apply an online change to a running VM.
/// This should be called at a safe point (between scan cycles).
pub fn apply_online_change(
    vm: &mut crate::vm::Vm,
    new_module: Module,
    analysis: &ChangeAnalysis,
) -> Result<(), String> {
    if !analysis.compatible {
        return Err(format!(
            "Incompatible change: {}",
            analysis.incompatible_reasons.join("; ")
        ));
    }

    // Save current retained locals before swap
    let old_module = vm.module().clone();
    let mut migrated_locals: HashMap<u16, Vec<Value>> = HashMap::new();

    for change in &analysis.function_changes {
        if change.kind == ChangeKind::Unchanged {
            continue;
        }

        // Find old and new function indices
        let old_func = old_module.find_function(&change.name);
        let new_func = new_module.find_function(&change.name);

        if let (Some((_, old_f)), Some((new_idx, new_f))) = (old_func, new_func) {
            if old_f.kind == PouKind::Program || old_f.kind == PouKind::FunctionBlock {
                // Get current retained locals for this function
                let current_locals = vm.get_retained_locals(&change.name);
                if let Some(old_locals) = current_locals {
                    let new_locals = migrate_locals(
                        &old_locals,
                        &old_f.locals,
                        &new_f.locals,
                    );
                    migrated_locals.insert(new_idx, new_locals);
                }
            }
        }
    }

    // Migrate globals
    let old_globals: Vec<Value> = (0..old_module.globals.slots.len())
        .map(|i| {
            vm.get_global(&old_module.globals.slots[i].name)
                .cloned()
                .unwrap_or(Value::Void)
        })
        .collect();
    let new_globals = migrate_locals(&old_globals, &old_module.globals, &new_module.globals);

    // Atomic swap: replace the module and restore state
    vm.swap_module(new_module, migrated_locals, new_globals);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_module(functions: Vec<Function>, globals: MemoryLayout) -> Module {
        Module {
            functions,
            globals,
            type_defs: vec![],
        }
    }

    fn make_func(name: &str, kind: PouKind, slots: Vec<(&str, VarType)>) -> Function {
        let mut layout = MemoryLayout::default();
        let mut offset = 0;
        for (n, ty) in &slots {
            let size = ty.size();
            layout.slots.push(VarSlot {
                name: n.to_string(),
                ty: *ty,
                offset,
                size,
                retain: false,
                int_width: IntWidth::None,
            });
            offset += size;
        }
        Function {
            name: name.to_string(),
            kind,
            register_count: 1,
            instructions: vec![Instruction::RetVoid],
            label_positions: vec![],
            locals: layout,
            source_map: vec![SourceLocation::default()],
            body_start_pc: 0,
        }
    }

    // =========================================================================
    // Compatibility analysis
    // =========================================================================

    #[test]
    fn identical_modules_are_compatible() {
        let m = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int)])],
            MemoryLayout::default(),
        );
        let analysis = analyze_change(&m, &m);
        assert!(analysis.compatible);
        assert!(analysis.incompatible_reasons.is_empty());
    }

    #[test]
    fn code_only_change_is_compatible() {
        let old = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int)])],
            MemoryLayout::default(),
        );
        let mut new_func = make_func("Main", PouKind::Program, vec![("x", VarType::Int)]);
        new_func.instructions.push(Instruction::Nop); // different code
        let new = make_module(vec![new_func], MemoryLayout::default());

        let analysis = analyze_change(&old, &new);
        assert!(analysis.compatible);
        assert!(analysis.function_changes[0].kind == ChangeKind::CodeOnly);
    }

    #[test]
    fn adding_variable_is_compatible() {
        let old = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int)])],
            MemoryLayout::default(),
        );
        let new = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int), ("y", VarType::Real)])],
            MemoryLayout::default(),
        );

        let analysis = analyze_change(&old, &new);
        assert!(analysis.compatible);
        assert!(analysis.new_vars.iter().any(|v| v.contains("y")));
        assert!(analysis.preserved_vars.iter().any(|v| v.contains("x")));
    }

    #[test]
    fn changing_variable_type_is_incompatible() {
        let old = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int)])],
            MemoryLayout::default(),
        );
        let new = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Real)])],
            MemoryLayout::default(),
        );

        let analysis = analyze_change(&old, &new);
        assert!(!analysis.compatible);
        assert!(analysis.incompatible_reasons.iter().any(|r| r.contains("changed type")));
    }

    #[test]
    fn removing_function_is_incompatible() {
        let old = make_module(
            vec![
                make_func("Main", PouKind::Program, vec![("x", VarType::Int)]),
                make_func("Helper", PouKind::Function, vec![]),
            ],
            MemoryLayout::default(),
        );
        let new = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int)])],
            MemoryLayout::default(),
        );

        let analysis = analyze_change(&old, &new);
        assert!(!analysis.compatible);
        assert!(analysis.incompatible_reasons.iter().any(|r| r.contains("removed")));
    }

    #[test]
    fn adding_new_function_is_compatible() {
        let old = make_module(
            vec![make_func("Main", PouKind::Program, vec![])],
            MemoryLayout::default(),
        );
        let new = make_module(
            vec![
                make_func("Main", PouKind::Program, vec![]),
                make_func("NewFunc", PouKind::Function, vec![]),
            ],
            MemoryLayout::default(),
        );

        let analysis = analyze_change(&old, &new);
        assert!(analysis.compatible);
        assert!(analysis.function_changes.iter().any(|c| c.name == "NewFunc" && c.kind == ChangeKind::New));
    }

    #[test]
    fn removing_variable_is_tracked() {
        let old = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int), ("y", VarType::Int)])],
            MemoryLayout::default(),
        );
        let new = make_module(
            vec![make_func("Main", PouKind::Program, vec![("x", VarType::Int)])],
            MemoryLayout::default(),
        );

        let analysis = analyze_change(&old, &new);
        assert!(analysis.removed_vars.iter().any(|v| v.contains("y")));
    }

    #[test]
    fn global_variable_type_change_is_incompatible() {
        let mut old_globals = MemoryLayout::default();
        old_globals.slots.push(VarSlot {
            name: "g".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None,
        });
        let mut new_globals = MemoryLayout::default();
        new_globals.slots.push(VarSlot {
            name: "g".into(), ty: VarType::Real, offset: 0, size: 8, retain: false, int_width: IntWidth::None,
        });

        let old = make_module(vec![make_func("Main", PouKind::Program, vec![])], old_globals);
        let new = make_module(vec![make_func("Main", PouKind::Program, vec![])], new_globals);

        let analysis = analyze_change(&old, &new);
        assert!(!analysis.compatible);
    }

    // =========================================================================
    // Variable migration
    // =========================================================================

    #[test]
    fn migrate_preserves_existing_vars() {
        let mut old_layout = MemoryLayout::default();
        old_layout.slots.push(VarSlot { name: "x".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None });
        old_layout.slots.push(VarSlot { name: "y".into(), ty: VarType::Real, offset: 8, size: 8, retain: false, int_width: IntWidth::None });

        let mut new_layout = MemoryLayout::default();
        new_layout.slots.push(VarSlot { name: "x".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None });
        new_layout.slots.push(VarSlot { name: "y".into(), ty: VarType::Real, offset: 8, size: 8, retain: false, int_width: IntWidth::None });
        new_layout.slots.push(VarSlot { name: "z".into(), ty: VarType::Bool, offset: 16, size: 1, retain: false, int_width: IntWidth::None });

        let old_locals = vec![Value::Int(42), Value::Real(1.5)];
        let new_locals = migrate_locals(&old_locals, &old_layout, &new_layout);

        assert_eq!(new_locals.len(), 3);
        assert_eq!(new_locals[0], Value::Int(42));     // preserved
        assert_eq!(new_locals[1], Value::Real(1.5));  // preserved
        assert_eq!(new_locals[2], Value::Bool(false));  // new default
    }

    #[test]
    fn migrate_with_reordered_vars() {
        let mut old_layout = MemoryLayout::default();
        old_layout.slots.push(VarSlot { name: "a".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None });
        old_layout.slots.push(VarSlot { name: "b".into(), ty: VarType::Int, offset: 8, size: 8, retain: false, int_width: IntWidth::None });

        let mut new_layout = MemoryLayout::default();
        new_layout.slots.push(VarSlot { name: "b".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None });
        new_layout.slots.push(VarSlot { name: "a".into(), ty: VarType::Int, offset: 8, size: 8, retain: false, int_width: IntWidth::None });

        let old_locals = vec![Value::Int(10), Value::Int(20)];
        let new_locals = migrate_locals(&old_locals, &old_layout, &new_layout);

        assert_eq!(new_locals[0], Value::Int(20)); // b was at index 1
        assert_eq!(new_locals[1], Value::Int(10)); // a was at index 0
    }

    #[test]
    fn migrate_with_removed_var() {
        let mut old_layout = MemoryLayout::default();
        old_layout.slots.push(VarSlot { name: "x".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None });
        old_layout.slots.push(VarSlot { name: "y".into(), ty: VarType::Int, offset: 8, size: 8, retain: false, int_width: IntWidth::None });

        let mut new_layout = MemoryLayout::default();
        new_layout.slots.push(VarSlot { name: "x".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None });

        let old_locals = vec![Value::Int(42), Value::Int(99)];
        let new_locals = migrate_locals(&old_locals, &old_layout, &new_layout);

        assert_eq!(new_locals.len(), 1);
        assert_eq!(new_locals[0], Value::Int(42)); // x preserved, y dropped
    }
}