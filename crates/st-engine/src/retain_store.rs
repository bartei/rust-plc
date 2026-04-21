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
    /// Struct/FB instance fields: program_name → instance_var_name → field_name → entry.
    /// Structs and FB instances store their field data in `fb_instances`, not in
    /// the locals vec, so they need a separate section in the snapshot.
    #[serde(default)]
    pub instance_fields:
        HashMap<String, HashMap<String, HashMap<String, RetainEntry>>>,
}

/// Capture the scalar fields of a struct instance from fb_instances.
fn capture_instance_fields(
    vm: &Vm,
    type_def_idx: u16,
    instance_key: &(u32, u16),
    retain: bool,
    persistent: bool,
) -> Option<HashMap<String, RetainEntry>> {
    let (_, fields) = vm.struct_type_fields(type_def_idx)?;
    let state = vm.fb_instances_ref().get(instance_key);
    let mut result = HashMap::new();
    for (j, field) in fields.iter().enumerate() {
        // Skip nested composite types for now
        if matches!(
            field.ty,
            VarType::FbInstance(_) | VarType::ClassInstance(_) | VarType::Struct(_) | VarType::Array(_)
        ) {
            continue;
        }
        let val = state
            .and_then(|s| s.get(j))
            .cloned()
            .unwrap_or(Value::default_for_type(field.ty));
        result.insert(
            field.name.clone(),
            RetainEntry {
                value: val,
                retain,
                persistent,
            },
        );
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
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
    let mut instance_fields: HashMap<String, HashMap<String, HashMap<String, RetainEntry>>> =
        HashMap::new();
    for (func_idx, locals) in vm.retained_locals_ref() {
        let func = &module.functions[*func_idx as usize];
        if func.kind != PouKind::Program {
            continue;
        }
        let caller_id = (0xFFFF_u32 << 16) | (*func_idx as u32);
        let mut vars = HashMap::new();
        for (j, slot) in func.locals.slots.iter().enumerate() {
            if !slot.retain && !slot.persistent {
                continue;
            }
            // Struct/FB instance fields are stored in fb_instances, not the
            // locals vec. Capture their fields into the instance_fields map.
            match slot.ty {
                VarType::Struct(td_idx) => {
                    let instance_key = (caller_id, j as u16);
                    if let Some(fields) = capture_instance_fields(
                        vm, td_idx, &instance_key, slot.retain, slot.persistent,
                    ) {
                        instance_fields
                            .entry(func.name.clone())
                            .or_default()
                            .insert(slot.name.clone(), fields);
                    }
                    continue;
                }
                VarType::FbInstance(fb_idx) => {
                    let fb_func = &module.functions[fb_idx as usize];
                    let instance_key = (caller_id, j as u16);
                    let state = vm.fb_instances_ref().get(&instance_key);
                    let mut fields = HashMap::new();
                    for (k, fb_slot) in fb_func.locals.slots.iter().enumerate() {
                        let val = state
                            .and_then(|s| s.get(k))
                            .cloned()
                            .unwrap_or(Value::default_for_type(fb_slot.ty));
                        if matches!(
                            fb_slot.ty,
                            VarType::FbInstance(_) | VarType::ClassInstance(_) | VarType::Struct(_) | VarType::Array(_)
                        ) {
                            continue;
                        }
                        fields.insert(
                            fb_slot.name.clone(),
                            RetainEntry {
                                value: val,
                                retain: slot.retain,
                                persistent: slot.persistent,
                            },
                        );
                    }
                    if !fields.is_empty() {
                        instance_fields
                            .entry(func.name.clone())
                            .or_default()
                            .insert(slot.name.clone(), fields);
                    }
                    continue;
                }
                VarType::ClassInstance(_) | VarType::Array(_) => {
                    continue;
                }
                _ => {}
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
        instance_fields,
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

    // Restore struct/FB instance fields from the instance_fields section.
    // These are stored in vm.fb_instances keyed by (caller_identity, slot).
    for (prog_name, instances) in &snapshot.instance_fields {
        if let Some((func_idx, _, slots)) =
            func_meta.iter().find(|(_, n, _)| n == prog_name)
        {
            let caller_id = (0xFFFF_u32 << 16) | (*func_idx as u32);
            for (inst_name, fields) in instances {
                // Check warm/cold filter on the first field entry
                let dominated = fields.values().next().is_some_and(|entry| {
                    if warm { entry.retain } else { entry.persistent }
                });
                if !dominated {
                    continue;
                }

                // Find the local slot for this instance variable
                let slot_info = slots.iter().find(|(_, n, _)| n == inst_name);
                let Some(&(slot_idx, _, slot_ty)) = slot_info else {
                    warnings.push(format!(
                        "{prog_name}.{inst_name}: no longer exists, skipped"
                    ));
                    continue;
                };

                let instance_key = (caller_id, slot_idx);

                match slot_ty {
                    VarType::Struct(td_idx) => {
                        if let Some((_, type_fields)) = vm.struct_type_fields(td_idx) {
                            let type_fields: Vec<(String, VarType)> = type_fields
                                .iter()
                                .map(|f| (f.name.clone(), f.ty))
                                .collect();
                            let mut instance_state: Vec<Value> = type_fields
                                .iter()
                                .map(|(_, ty)| Value::default_for_type(*ty))
                                .collect();
                            // Carry over existing state if present
                            if let Some(existing) = vm.fb_instances_ref().get(&instance_key) {
                                for (i, val) in existing.iter().enumerate() {
                                    if i < instance_state.len() {
                                        instance_state[i] = val.clone();
                                    }
                                }
                            }
                            for (field_name, entry) in fields {
                                if let Some(pos) = type_fields
                                    .iter()
                                    .position(|(n, _)| n == field_name)
                                {
                                    let field_ty = type_fields[pos].1;
                                    if is_compatible(&entry.value, field_ty) {
                                        instance_state[pos] = entry.value.clone();
                                    } else {
                                        warnings.push(format!(
                                            "{prog_name}.{inst_name}.{field_name}: type mismatch, skipped"
                                        ));
                                    }
                                } else {
                                    warnings.push(format!(
                                        "{prog_name}.{inst_name}.{field_name}: no longer exists, skipped"
                                    ));
                                }
                            }
                            vm.set_fb_instance(instance_key, instance_state);
                        }
                    }
                    VarType::FbInstance(fb_idx) => {
                        let fb_slots: Vec<(String, VarType)> = vm
                            .module()
                            .functions[fb_idx as usize]
                            .locals
                            .slots
                            .iter()
                            .map(|s| (s.name.clone(), s.ty))
                            .collect();
                        let mut instance_state: Vec<Value> = fb_slots
                            .iter()
                            .map(|(_, ty)| Value::default_for_type(*ty))
                            .collect();
                        if let Some(existing) = vm.fb_instances_ref().get(&instance_key) {
                            for (i, val) in existing.iter().enumerate() {
                                if i < instance_state.len() {
                                    instance_state[i] = val.clone();
                                }
                            }
                        }
                        for (field_name, entry) in fields {
                            if let Some(pos) = fb_slots
                                .iter()
                                .position(|(n, _)| n == field_name)
                            {
                                let field_ty = fb_slots[pos].1;
                                if is_compatible(&entry.value, field_ty) {
                                    instance_state[pos] = entry.value.clone();
                                } else {
                                    warnings.push(format!(
                                        "{prog_name}.{inst_name}.{field_name}: type mismatch, skipped"
                                    ));
                                }
                            } else {
                                warnings.push(format!(
                                    "{prog_name}.{inst_name}.{field_name}: no longer exists, skipped"
                                ));
                            }
                        }
                        vm.set_fb_instance(instance_key, instance_state);
                    }
                    _ => {
                        warnings.push(format!(
                            "{prog_name}.{inst_name}: not a struct/FB, skipped instance fields"
                        ));
                    }
                }
            }
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
