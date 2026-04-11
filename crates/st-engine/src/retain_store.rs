//! Non-volatile persistence for RETAIN and PERSISTENT variables.
//!
//! IEC 61131-3 defines two retention qualifiers:
//! - **RETAIN**: survives warm restart (process restart without re-download)
//! - **PERSISTENT**: survives cold restart (program re-download)
//! - **RETAIN PERSISTENT**: survives both
//!
//! This module captures flagged variables from the VM, serializes them to
//! a JSON file, and restores them on engine startup. Name-based keying
//! ensures the file survives minor program changes where variable order
//! shifts but names/types are preserved.

use crate::vm::Vm;
use serde::{Deserialize, Serialize};
use st_ir::{PouKind, Value, VarType};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuration for retain variable storage.
#[derive(Debug, Clone)]
pub struct RetainConfig {
    /// Path to the retain file (e.g., `.st-retain/Main.retain`).
    pub path: PathBuf,
    /// Save a checkpoint every N scan cycles. 0 = only on explicit save.
    pub checkpoint_cycles: u32,
}

/// A single retained variable entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetainEntry {
    pub value: Value,
    pub retain: bool,
    pub persistent: bool,
}

/// Serializable snapshot of all retainable VM state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetainSnapshot {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// Timestamp (Unix epoch seconds) when the snapshot was created.
    pub created_at: u64,
    /// Global variables: name → entry (only those with retain/persistent flag).
    pub globals: HashMap<String, RetainEntry>,
    /// Program locals: program_name → (var_name → entry).
    pub program_locals: HashMap<String, HashMap<String, RetainEntry>>,
}

/// Capture all retainable variables from the VM into a snapshot.
///
/// Includes every variable where `retain == true` OR `persistent == true`.
/// The restore side filters by warm/cold restart semantics.
pub fn capture_snapshot(vm: &Vm) -> RetainSnapshot {
    let module = vm.module();
    let mut globals = HashMap::new();

    // Capture retainable globals
    for (i, slot) in module.globals.slots.iter().enumerate() {
        if !slot.retain && !slot.persistent {
            continue;
        }
        let val = vm.globals_ref().get(i).cloned().unwrap_or(Value::Void);
        globals.insert(
            slot.name.clone(),
            RetainEntry {
                value: val,
                retain: slot.retain,
                persistent: slot.persistent,
            },
        );
    }

    // Capture retainable program locals
    let mut program_locals = HashMap::new();
    for (func_idx, locals) in vm.retained_locals_ref() {
        let func = &module.functions[*func_idx as usize];
        if func.kind != PouKind::Program {
            continue;
        }
        let mut vars = HashMap::new();
        for (j, slot) in func.locals.slots.iter().enumerate() {
            if !slot.retain && !slot.persistent {
                continue;
            }
            // Skip composite types (FBs, structs, classes) — their fields
            // are stored separately in fb_instances, not in the locals vec.
            if matches!(
                slot.ty,
                VarType::FbInstance(_) | VarType::ClassInstance(_) | VarType::Struct(_)
            ) {
                continue;
            }
            let val = locals.get(j).cloned().unwrap_or(Value::Void);
            vars.insert(
                slot.name.clone(),
                RetainEntry {
                    value: val,
                    retain: slot.retain,
                    persistent: slot.persistent,
                },
            );
        }
        if !vars.is_empty() {
            program_locals.insert(func.name.clone(), vars);
        }
    }

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    RetainSnapshot {
        version: 1,
        created_at,
        globals,
        program_locals,
    }
}

/// Restore retained variables into a VM.
///
/// - `warm = true`: warm restart — restores entries where `retain == true`
///   (covers RETAIN and RETAIN PERSISTENT).
/// - `warm = false`: cold restart — restores entries where `persistent == true`
///   (covers PERSISTENT and RETAIN PERSISTENT).
///
/// Returns a list of warnings for variables that could not be restored
/// (type mismatch, no longer exists, etc.).
pub fn restore_snapshot(vm: &mut Vm, snapshot: &RetainSnapshot, warm: bool) -> Vec<String> {
    let mut warnings = Vec::new();

    // We need module metadata for slot lookup, but also &mut vm for setting
    // values. Clone the minimal metadata (slot names/types/indices) first.
    let global_slots: Vec<(u16, String, VarType)> = vm
        .module()
        .globals
        .slots
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u16, s.name.clone(), s.ty))
        .collect();

    type SlotMeta = (u16, String, VarType);
    let func_meta: Vec<(u16, String, Vec<SlotMeta>)> = vm
        .module()
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| f.kind == PouKind::Program)
        .map(|(idx, f)| {
            let slots: Vec<(u16, String, VarType)> = f
                .locals
                .slots
                .iter()
                .enumerate()
                .map(|(j, s)| (j as u16, s.name.clone(), s.ty))
                .collect();
            (idx as u16, f.name.clone(), slots)
        })
        .collect();

    // Restore globals
    for (name, entry) in &snapshot.globals {
        let dominated = if warm { entry.retain } else { entry.persistent };
        if !dominated {
            continue;
        }
        if let Some(&(slot_idx, _, slot_ty)) =
            global_slots.iter().find(|(_, n, _)| n == name)
        {
            if is_compatible(&entry.value, slot_ty) {
                vm.set_global_unchecked(slot_idx, entry.value.clone());
            } else {
                warnings.push(format!("Global '{name}': type mismatch, skipped"));
            }
        } else {
            warnings.push(format!("Global '{name}': no longer exists, skipped"));
        }
    }

    // Restore program locals
    for (prog_name, vars) in &snapshot.program_locals {
        if let Some((func_idx, _, slots)) =
            func_meta.iter().find(|(_, n, _)| n == prog_name)
        {
            // Start with defaults for all slots
            let num_slots = vm.module().functions[*func_idx as usize].locals.slots.len();
            let mut locals: Vec<Value> = vm.module().functions[*func_idx as usize]
                .locals
                .slots
                .iter()
                .map(|s| Value::default_for_type(s.ty))
                .collect();

            // Also carry over any existing retained locals (non-retain vars
            // may already have state from prior cycles).
            if let Some(existing) = vm.retained_locals_ref().get(func_idx) {
                for (i, val) in existing.iter().enumerate() {
                    if i < locals.len() {
                        locals[i] = val.clone();
                    }
                }
            }

            for (var_name, entry) in vars {
                let dominated = if warm { entry.retain } else { entry.persistent };
                if !dominated {
                    continue;
                }
                if let Some(&(slot_idx, _, slot_ty)) =
                    slots.iter().find(|(_, n, _)| n == var_name)
                {
                    if is_compatible(&entry.value, slot_ty) {
                        if (slot_idx as usize) < num_slots {
                            locals[slot_idx as usize] = entry.value.clone();
                        }
                    } else {
                        warnings.push(format!(
                            "{prog_name}.{var_name}: type mismatch, skipped"
                        ));
                    }
                } else {
                    warnings.push(format!(
                        "{prog_name}.{var_name}: no longer exists, skipped"
                    ));
                }
            }

            vm.set_retained_locals(*func_idx, locals);
        }
    }

    warnings
}

/// Save a snapshot to disk as JSON. Uses atomic write (tmp + rename)
/// to prevent corruption if the process is killed mid-write.
pub fn save_to_file(snapshot: &RetainSnapshot, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create retain dir {}: {e}", parent.display()))?;
    }

    let tmp_path = path.with_extension("retain.tmp");
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("Serialize error: {e}"))?;

    std::fs::write(&tmp_path, json.as_bytes())
        .map_err(|e| format!("Write error: {e}"))?;

    std::fs::rename(&tmp_path, path)
        .map_err(|e| format!("Rename error: {e}"))?;

    Ok(())
}

/// Load a snapshot from a JSON file on disk.
pub fn load_from_file(path: &Path) -> Result<RetainSnapshot, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;

    serde_json::from_str(&content)
        .map_err(|e| format!("Deserialize error: {e}"))
}

/// Check whether a saved Value is compatible with a VarType for restore.
fn is_compatible(val: &Value, ty: VarType) -> bool {
    matches!(
        (val, ty),
        (Value::Bool(_), VarType::Bool)
            | (Value::Int(_), VarType::Int)
            | (Value::UInt(_), VarType::UInt)
            | (Value::Real(_), VarType::Real)
            | (Value::String(_), VarType::String)
            | (Value::Time(_), VarType::Time)
    )
}
