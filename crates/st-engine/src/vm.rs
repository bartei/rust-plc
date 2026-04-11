//! Bytecode VM: fetch-decode-execute interpreter for the register-based IR.

use crate::debug::{self, DebugState, FrameInfo, VariableInfo};
use st_ir::*;

/// Runtime error during VM execution.
#[derive(Debug, thiserror::Error)]
pub enum VmError {
    #[error("division by zero")]
    DivisionByZero,
    #[error("stack overflow (max depth {0})")]
    StackOverflow(usize),
    #[error("invalid function index {0}")]
    InvalidFunction(u16),
    #[error("invalid label {0}")]
    InvalidLabel(u32),
    #[error("execution limit exceeded ({0} instructions)")]
    ExecutionLimit(u64),
    #[error("halt")]
    Halt,
}

/// Configuration for the VM.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Maximum call stack depth.
    pub max_call_depth: usize,
    /// Maximum instructions per execution (0 = unlimited).
    pub max_instructions: u64,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_call_depth: 256,
            max_instructions: 10_000_000,
        }
    }
}

/// A stack frame for function calls.
#[derive(Debug)]
struct CallFrame {
    func_index: u16,
    registers: Vec<Value>,
    locals: Vec<Value>,
    pc: usize,
    /// Register to store the return value in the caller's frame.
    return_reg: Option<Reg>,
    /// For FB instances: the instance slot in the caller's frame (for state persistence).
    instance_slot: Option<u16>,
}

/// The virtual machine state.
pub struct Vm {
    config: VmConfig,
    /// The compiled module.
    module: Module,
    /// Global variable storage.
    globals: Vec<Value>,
    /// Call stack.
    call_stack: Vec<CallFrame>,
    /// Total instructions executed (for limit checking).
    instruction_count: u64,
    /// Debug state (breakpoints, stepping, etc.).
    debug: DebugState,
    /// Retained local variables for PROGRAM POUs (persists across scan cycles).
    retained_locals: std::collections::HashMap<u16, Vec<Value>>,
    /// Forced variables: name → forced value. Overrides runtime values.
    forced_variables: std::collections::HashMap<String, Value>,
    /// Fast lookup: slot indices of currently-forced GLOBAL variables.
    /// Kept in sync with `forced_variables` for global names. Used by the
    /// hot-path write blockers (`set_global_by_slot`, `StoreGlobal`) to
    /// reject device updates and program writes to forced variables —
    /// matching real-PLC force semantics where the forced value takes
    /// precedence over all other writers.
    forced_global_slots: std::collections::HashSet<u16>,
    /// Cumulative elapsed time in nanoseconds (updated by engine each cycle).
    elapsed_time_ms: i64,
    /// FB/class instance storage: (caller_identity, instance_slot) → locals.
    /// caller_identity encodes both the function index and the caller's own
    /// instance position to distinguish nested instances (e.g. class inside FB).
    fb_instances: std::collections::HashMap<(u32, u16), Vec<Value>>,
}

impl Vm {
    /// Create a new VM with the given module.
    pub fn new(module: Module, config: VmConfig) -> Self {
        let globals = module
            .globals
            .slots
            .iter()
            .map(|s| Value::default_for_type(s.ty))
            .collect();
        Self {
            config,
            module,
            globals,
            call_stack: Vec::new(),
            instruction_count: 0,
            debug: DebugState::new(),
            retained_locals: std::collections::HashMap::new(),
            forced_variables: std::collections::HashMap::new(),
            forced_global_slots: std::collections::HashSet::new(),
            elapsed_time_ms: 0,
            fb_instances: std::collections::HashMap::new(),
        }
    }

    /// Run a function by name. Returns the return value (or Void for programs).
    /// For PROGRAM POUs, local variables are retained across calls (PLC behavior).
    pub fn run(&mut self, func_name: &str) -> Result<Value, VmError> {
        let (func_index, func) = self
            .module
            .find_function(func_name)
            .ok_or(VmError::InvalidFunction(0))?;

        let register_count = func.register_count as usize;
        let is_program = func.kind == st_ir::PouKind::Program;

        // For PROGRAM POUs, reuse retained locals and skip init code
        let (locals, start_pc) = if is_program {
            if let Some(retained) = self.retained_locals.get(&func_index) {
                (retained.clone(), func.body_start_pc)
            } else {
                (func.locals.slots.iter().map(|s| Value::default_for_type(s.ty)).collect(), 0)
            }
        } else {
            (func.locals.slots.iter().map(|s| Value::default_for_type(s.ty)).collect(), 0)
        };

        self.call_stack.push(CallFrame {
            func_index,
            registers: vec![Value::default(); register_count.max(1)],
            locals,
            pc: start_pc,
            return_reg: None,
            instance_slot: None,
        });

        self.execute()
    }

    /// Continue execution from a halted state (after a debug pause).
    /// Returns when the VM halts again or the function completes.
    pub fn continue_execution(&mut self) -> Result<Value, VmError> {
        if self.call_stack.is_empty() {
            return Ok(Value::Void);
        }
        self.execute()
    }

    /// Run a single scan cycle: execute the named program once.
    pub fn scan_cycle(&mut self, program_name: &str) -> Result<(), VmError> {
        self.run(program_name)?;
        Ok(())
    }

    /// Run the synthetic `__global_init` function generated by the compiler
    /// to apply `VAR_GLOBAL x : T := <expr>;` initial values. Called once
    /// by the engine right after VM construction. No-op if the function is
    /// absent (modules with no global initializers, or modules compiled
    /// before this feature existed).
    pub fn run_global_init(&mut self) -> Result<(), VmError> {
        if self.module.find_function("__global_init").is_none() {
            return Ok(());
        }
        self.run("__global_init")?;
        Ok(())
    }

    /// Get a global variable value by name.
    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.module
            .globals
            .find_slot(name)
            .map(|(i, _)| &self.globals[i as usize])
    }

    /// Set a global variable value by name.
    pub fn set_global(&mut self, name: &str, value: Value) {
        if let Some((i, slot)) = self.module.globals.find_slot(name) {
            // Forced globals reject all writes — devices, program code,
            // online change, etc. The forced value persists until unforce.
            if self.forced_global_slots.contains(&i) {
                return;
            }
            let width = slot.int_width;
            self.globals[i as usize] = narrow_value(value, width);
        }
    }

    /// Set a global variable value by slot index.
    /// Forced slots silently drop the write — this is what makes a force
    /// stick across cycles even when `comm.read_inputs` is pushing fresh
    /// device data on every scan.
    pub fn set_global_by_slot(&mut self, slot: u16, value: Value) {
        if (slot as usize) >= self.globals.len() {
            return;
        }
        if self.forced_global_slots.contains(&slot) {
            return;
        }
        let width = self
            .module
            .globals
            .slots
            .get(slot as usize)
            .map(|s| s.int_width)
            .unwrap_or(IntWidth::None);
        self.globals[slot as usize] = narrow_value(value, width);
    }

    /// Get a global variable value by slot index.
    pub fn get_global_by_slot(&self, slot: u16) -> Option<&Value> {
        self.globals.get(slot as usize)
    }

    /// Get total instructions executed.
    pub fn instruction_count(&self) -> u64 {
        self.instruction_count
    }

    /// Get retained locals for a function by name.
    pub fn get_retained_locals(&self, func_name: &str) -> Option<Vec<Value>> {
        let (idx, _) = self.module.find_function(func_name)?;
        self.retained_locals.get(&idx).cloned()
    }

    /// Force a variable to a specific value. Real-PLC semantics: the
    /// forced value takes precedence over device updates AND program
    /// writes. Implemented by writing the value INTO the underlying slot
    /// (so every reader naturally sees it) and adding the slot to
    /// `forced_global_slots` so future writes from any source are
    /// silently dropped. Locals/program-locals are still tracked via the
    /// name-keyed map for the LoadLocal overlay path.
    pub fn force_variable(&mut self, name: &str, value: Value) {
        // Narrow the forced value to the slot's declared integer width
        // (so forcing a SINT to 200 wraps to -56 instead of being stored
        // as a wide INT — matching what the running program would see).
        if let Some((slot, slot_def)) = self.module.globals.find_slot(name) {
            let narrowed = narrow_value(value.clone(), slot_def.int_width);
            self.forced_variables
                .insert(name.to_uppercase(), narrowed.clone());
            self.forced_global_slots.insert(slot);
            if (slot as usize) < self.globals.len() {
                self.globals[slot as usize] = narrowed;
            }
        } else {
            // Not a global — fall back to the name-keyed map only.
            self.forced_variables.insert(name.to_uppercase(), value);
        }
    }

    /// Remove a force on a variable. The slot keeps whatever the forced
    /// value was until the next write from a device or program — i.e.
    /// behavior immediately returns to normal on the next scan cycle.
    pub fn unforce_variable(&mut self, name: &str) {
        self.forced_variables.remove(&name.to_uppercase());
        if let Some((slot, _)) = self.module.globals.find_slot(name) {
            self.forced_global_slots.remove(&slot);
        }
    }

    /// Get all currently forced variables.
    pub fn forced_variables(&self) -> &std::collections::HashMap<String, Value> {
        &self.forced_variables
    }

    /// Set the cumulative elapsed time (called by engine before each scan cycle).
    pub fn set_elapsed_time_ms(&mut self, ns: i64) {
        self.elapsed_time_ms = ns;
    }

    /// Get the cumulative elapsed time in nanoseconds.
    pub fn elapsed_time_ms(&self) -> i64 {
        self.elapsed_time_ms
    }

    /// Atomically swap the module and restore migrated state.
    /// Must be called when the VM is not executing (between scan cycles).
    pub fn swap_module(
        &mut self,
        new_module: Module,
        migrated_locals: std::collections::HashMap<u16, Vec<Value>>,
        new_globals: Vec<Value>,
    ) {
        self.module = new_module;
        self.globals = new_globals;
        self.retained_locals = migrated_locals;
        self.call_stack.clear();
    }

    /// Clear the call stack (for clean recovery after debug detach).
    pub fn clear_call_stack(&mut self) {
        self.call_stack.clear();
    }

    /// Reset instruction counter.
    pub fn reset_instruction_count(&mut self) {
        self.instruction_count = 0;
    }

    /// Get a mutable reference to the debug state.
    pub fn debug_mut(&mut self) -> &mut DebugState {
        &mut self.debug
    }

    /// Get the debug state.
    pub fn debug_state(&self) -> &DebugState {
        &self.debug
    }

    /// Get the module reference.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// Get struct field layout from type_defs. Returns None if not a struct.
    pub fn struct_type_fields(&self, type_def_idx: u16) -> Option<(&str, &[VarSlot])> {
        self.module.type_defs.get(type_def_idx as usize).and_then(|td| {
            if let TypeDef::Struct { name, fields } = td {
                Some((name.as_str(), fields.as_slice()))
            } else {
                None
            }
        })
    }

    /// Get the call stack as frame info for the debugger.
    pub fn stack_frames(&self) -> Vec<FrameInfo> {
        self.call_stack
            .iter()
            .rev()
            .map(|frame| {
                let func = &self.module.functions[frame.func_index as usize];
                // When halted by debugger, PC points to the next instruction to execute.
                // Use PC directly (not PC-1) to get the source location of where we stopped.
                // If PC is past the end, use the last instruction.
                let sm_pc = frame.pc.min(func.source_map.len().saturating_sub(1));
                let (source_offset, source_end) = func
                    .source_map
                    .get(sm_pc)
                    .map(|sm| (sm.byte_offset, sm.byte_end))
                    .unwrap_or((0, 0));
                FrameInfo {
                    func_index: frame.func_index,
                    func_name: func.name.clone(),
                    pc: frame.pc,
                    source_offset,
                    source_end,
                }
            })
            .collect()
    }

    /// Get local variables for the current (topmost) frame.
    pub fn current_locals(&self) -> Vec<VariableInfo> {
        let Some(frame) = self.call_stack.last() else {
            return Vec::new();
        };
        let func = &self.module.functions[frame.func_index as usize];
        func.locals
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let value = frame
                    .locals
                    .get(i)
                    .cloned()
                    .unwrap_or(Value::Void);
                VariableInfo {
                    name: slot.name.clone(),
                    value: debug::format_value(&value),
                    ty: debug::format_var_type_with_width(slot.ty, slot.int_width)
                        .to_string(),
                    var_ref: 0,
                }
            })
            .collect()
    }

    /// Get global variables.
    pub fn global_variables(&self) -> Vec<VariableInfo> {
        self.module
            .globals
            .slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let value = self.globals.get(i).cloned().unwrap_or(Value::Void);
                VariableInfo {
                    name: slot.name.clone(),
                    value: debug::format_value(&value),
                    ty: debug::format_var_type_with_width(slot.ty, slot.int_width)
                        .to_string(),
                    var_ref: 0,
                }
            })
            .collect()
    }

    /// Catalog of every variable that the monitor panel COULD watch — names
    /// and types only, no values. Includes globals + every PROGRAM POU's
    /// declared local variables (regardless of whether the program has
    /// actually run yet). Used by the Monitor panel to populate its
    /// autocomplete at launch time, before the first scan cycle has
    /// populated `retained_locals`.
    pub fn monitorable_catalog(&self) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = self
            .module
            .globals
            .slots
            .iter()
            .map(|slot| {
                (
                    slot.name.clone(),
                    debug::format_var_type_with_width(slot.ty, slot.int_width)
                        .to_string(),
                )
            })
            .collect();
        for func in &self.module.functions {
            if func.kind != st_ir::PouKind::Program {
                continue;
            }
            let prefix = &func.name;
            for slot in &func.locals.slots {
                // Skip FB/class instance slots — they're not scalar values.
                // Their FIELDS are enumerated via the recursion below.
                match slot.ty {
                    VarType::FbInstance(fb_idx) => {
                        let inst_prefix = format!("{prefix}.{}", slot.name);
                        self.catalog_fb_fields(fb_idx, &inst_prefix, &mut result);
                    }
                    VarType::ClassInstance(_) => { /* skip */ }
                    VarType::Struct(td_idx) => {
                        let inst_prefix = format!("{prefix}.{}", slot.name);
                        self.catalog_struct_fields(td_idx, &inst_prefix, &mut result);
                    }
                    _ => {
                        result.push((
                            format!("{prefix}.{}", slot.name),
                            debug::format_var_type_with_width(slot.ty, slot.int_width)
                                .to_string(),
                        ));
                    }
                }
            }
        }
        result
    }

    /// Enumerate a struct's fields for the catalog (schema only, no values).
    fn catalog_struct_fields(
        &self,
        type_def_idx: u16,
        prefix: &str,
        result: &mut Vec<(String, String)>,
    ) {
        if let Some((_, fields)) = self.struct_type_fields(type_def_idx) {
            for field in fields {
                result.push((
                    format!("{prefix}.{}", field.name),
                    debug::format_var_type_with_width(field.ty, field.int_width)
                        .to_string(),
                ));
            }
        }
    }

    /// Recursively snapshot a FB instance's fields with runtime values.
    /// `instance_key` is the key in `fb_instances` for this FB instance.
    /// `nested_caller` is the caller_identity to use when looking up nested
    /// FB instances WITHIN this FB.
    fn snapshot_fb_fields(
        &self,
        fb_func_idx: u16,
        prefix: &str,
        instance_key: &(u32, u16),
        nested_caller: u32,
        result: &mut Vec<VariableInfo>,
    ) {
        let fb_func = &self.module.functions[fb_func_idx as usize];
        let fb_state = self.fb_instances.get(instance_key);
        for (j, fb_slot) in fb_func.locals.slots.iter().enumerate() {
            match fb_slot.ty {
                VarType::FbInstance(nested_fb_idx) => {
                    // Recurse into nested FB (e.g., CTU inside FillController)
                    let nested_prefix = format!("{prefix}.{}", fb_slot.name);
                    let nested_key = (nested_caller, j as u16);
                    let deeper_caller =
                        ((j as u32) << 16) | (nested_fb_idx as u32);
                    self.snapshot_fb_fields(
                        nested_fb_idx,
                        &nested_prefix,
                        &nested_key,
                        deeper_caller,
                        result,
                    );
                }
                VarType::ClassInstance(_) => { /* skip */ }
                _ => {
                    let fb_value = fb_state
                        .and_then(|s| s.get(j))
                        .cloned()
                        .unwrap_or(Value::Void);
                    result.push(VariableInfo {
                        name: format!("{prefix}.{}", fb_slot.name),
                        value: debug::format_value(&fb_value),
                        ty: debug::format_var_type_with_width(
                            fb_slot.ty,
                            fb_slot.int_width,
                        )
                        .to_string(),
                        var_ref: 0,
                    });
                }
            }
        }
    }

    /// Recursively enumerate a FB's fields for the catalog (schema only,
    /// no runtime values). Handles nested FBs (e.g., CTU inside a controller).
    fn catalog_fb_fields(
        &self,
        fb_func_idx: u16,
        prefix: &str,
        result: &mut Vec<(String, String)>,
    ) {
        let fb_func = &self.module.functions[fb_func_idx as usize];
        for fb_slot in &fb_func.locals.slots {
            match fb_slot.ty {
                VarType::FbInstance(nested_idx) => {
                    // Recurse into nested FB, don't add the FB slot itself
                    let nested_prefix = format!("{prefix}.{}", fb_slot.name);
                    self.catalog_fb_fields(nested_idx, &nested_prefix, result);
                }
                VarType::ClassInstance(_) => { /* skip */ }
                _ => {
                    result.push((
                        format!("{prefix}.{}", fb_slot.name),
                        debug::format_var_type_with_width(fb_slot.ty, fb_slot.int_width)
                            .to_string(),
                    ));
                }
            }
        }
    }

    /// All variables suitable for live monitoring: globals + PROGRAM retained
    /// locals + FB instance fields. PROGRAM locals persist across scan
    /// cycles (they're saved in `retained_locals` at cycle end), and FB
    /// instance state is saved in `fb_instances`. Together these form the
    /// complete set a PLC monitor panel would display.
    pub fn monitorable_variables(&self) -> Vec<VariableInfo> {
        let mut result = self.global_variables();
        // Add retained locals for every PROGRAM POU.
        for (func_idx, locals) in &self.retained_locals {
            let func = &self.module.functions[*func_idx as usize];
            if func.kind != st_ir::PouKind::Program {
                continue;
            }
            let prefix = &func.name;
            // Compute the PROGRAM's caller identity so we can look up its
            // FB instances in the fb_instances map.
            let caller_id = (0xFFFF_u32 << 16) | (*func_idx as u32);

            for (i, slot) in func.locals.slots.iter().enumerate() {
                // Skip FB/class instance slots — their FIELDS are
                // enumerated via the recursion below.
                match slot.ty {
                    VarType::FbInstance(fb_idx) => {
                        let inst_prefix = format!("{prefix}.{}", slot.name);
                        let instance_key = (caller_id, i as u16);
                        let nested_caller = ((i as u32) << 16) | (fb_idx as u32);
                        self.snapshot_fb_fields(
                            fb_idx,
                            &inst_prefix,
                            &instance_key,
                            nested_caller,
                            &mut result,
                        );
                    }
                    VarType::ClassInstance(_) => { /* skip */ }
                    VarType::Struct(td_idx) => {
                        let inst_prefix = format!("{prefix}.{}", slot.name);
                        let instance_key = (caller_id, i as u16);
                        self.snapshot_struct_fields(
                            td_idx,
                            &inst_prefix,
                            &instance_key,
                            &mut result,
                        );
                    }
                    _ => {
                        let value = locals
                            .get(i)
                            .cloned()
                            .unwrap_or(Value::Void);
                        result.push(VariableInfo {
                            name: format!("{prefix}.{}", slot.name),
                            value: debug::format_value(&value),
                            ty: debug::format_var_type_with_width(slot.ty, slot.int_width)
                                .to_string(),
                            var_ref: 0,
                        });
                    }
                }
            }
        }
        result
    }

    /// Snapshot a struct instance's fields with runtime values.
    fn snapshot_struct_fields(
        &self,
        type_def_idx: u16,
        prefix: &str,
        instance_key: &(u32, u16),
        result: &mut Vec<VariableInfo>,
    ) {
        if let Some((_, fields)) = self.struct_type_fields(type_def_idx) {
            let state = self.fb_instances.get(instance_key);
            for (j, field) in fields.iter().enumerate() {
                let value = state
                    .and_then(|s| s.get(j))
                    .cloned()
                    .unwrap_or(Value::default_for_type(field.ty));
                result.push(VariableInfo {
                    name: format!("{prefix}.{}", field.name),
                    value: debug::format_value(&value),
                    ty: debug::format_var_type_with_width(field.ty, field.int_width)
                        .to_string(),
                    var_ref: 0,
                });
            }
        }
    }

    /// Resolve a dotted FB field path like "counter.Q" or "counter.CV"
    /// from the current call frame's context. Returns the field value if
    /// found, or None if the path doesn't resolve.
    ///
    /// This is used by the DAP evaluate handler when paused inside a FB
    /// body — the user hovers over `counter.Q` and expects to see the
    /// CTU's Q output, not `<unknown>`.
    pub fn resolve_fb_field(&self, path: &str) -> Option<VariableInfo> {
        let dot = path.find('.')?;
        let obj_name = &path[..dot];
        let field_name = &path[dot + 1..];

        let frame = self.call_stack.last()?;
        let func = &self.module.functions[frame.func_index as usize];

        // Find the local slot for the object (e.g., "counter" or "stats")
        let (slot_idx, slot) = func.locals.find_slot(obj_name)?;

        let caller_id = self.caller_identity();
        let instance_key = (caller_id, slot_idx);

        // Handle both FbInstance and Struct types
        let (field_idx, field_slot) = match slot.ty {
            VarType::FbInstance(fb_idx) => {
                let fb_func = &self.module.functions[fb_idx as usize];
                fb_func.locals.find_slot(field_name)?
            }
            VarType::Struct(td_idx) => {
                let (_, fields) = self.struct_type_fields(td_idx)?;
                fields.iter()
                    .enumerate()
                    .find(|(_, f)| f.name.eq_ignore_ascii_case(field_name))
                    .map(|(i, f)| (i as u16, f))?
            }
            _ => return None,
        };

        // Look up the instance state using the current frame's identity.
        // If the instance hasn't been used yet, fb_instances has no entry —
        // return the default value rather than None so the debugger can
        // still display the field (with a "0" / "FALSE" / "VOID" value).
        let fb_state = self.fb_instances.get(&instance_key);

        let value = fb_state
            .and_then(|s| s.get(field_idx as usize))
            .cloned()
            .unwrap_or(Value::default_for_type(field_slot.ty));

        Some(VariableInfo {
            name: path.to_string(),
            value: debug::format_value(&value),
            ty: debug::format_var_type_with_width(field_slot.ty, field_slot.int_width)
                .to_string(),
            var_ref: 0,
        })
    }

    /// Enhanced `current_locals()` that also includes FB instance fields
    /// as expandable sub-entries. For each FbInstance local in the current
    /// frame, the actual field values are included with dotted names
    /// (e.g., "counter.Q", "counter.CV" alongside the regular locals).
    pub fn current_locals_with_fb_fields(&self) -> Vec<VariableInfo> {
        let Some(frame) = self.call_stack.last() else {
            return Vec::new();
        };
        let func = &self.module.functions[frame.func_index as usize];
        let caller_id = self.caller_identity();
        let mut result = Vec::new();

        for (i, slot) in func.locals.slots.iter().enumerate() {
            match slot.ty {
                VarType::FbInstance(fb_idx) => {
                    // Don't show the FB slot itself (meaningless value).
                    // Instead show its fields with dotted names.
                    let fb_func = &self.module.functions[fb_idx as usize];
                    let instance_key = (caller_id, i as u16);
                    let fb_state = self.fb_instances.get(&instance_key);
                    for (j, fb_slot) in fb_func.locals.slots.iter().enumerate() {
                        // Skip nested FBs (show their fields recursively would
                        // need more levels — for now just show scalar fields)
                        if matches!(fb_slot.ty, VarType::FbInstance(_) | VarType::ClassInstance(_)) {
                            continue;
                        }
                        let fb_value = fb_state
                            .and_then(|s| s.get(j))
                            .cloned()
                            .unwrap_or(Value::Void);
                        result.push(VariableInfo {
                            name: format!("{}.{}", slot.name, fb_slot.name),
                            value: debug::format_value(&fb_value),
                            ty: debug::format_var_type_with_width(fb_slot.ty, fb_slot.int_width)
                                .to_string(),
                            var_ref: 0,
                        });
                    }
                }
                VarType::ClassInstance(_) => { /* skip for now */ }
                VarType::Struct(td_idx) => {
                    // Show struct fields with dotted names.
                    if let Some((_, fields)) = self.struct_type_fields(td_idx) {
                        let instance_key = (caller_id, i as u16);
                        let state = self.fb_instances.get(&instance_key);
                        for (j, field) in fields.iter().enumerate() {
                            let value = state
                                .and_then(|s| s.get(j))
                                .cloned()
                                .unwrap_or(Value::default_for_type(field.ty));
                            result.push(VariableInfo {
                                name: format!("{}.{}", slot.name, field.name),
                                value: debug::format_value(&value),
                                ty: debug::format_var_type_with_width(field.ty, field.int_width)
                                    .to_string(),
                                var_ref: 0,
                            });
                        }
                    }
                }
                _ => {
                    let value = frame
                        .locals
                        .get(i)
                        .cloned()
                        .unwrap_or(Value::Void);
                    result.push(VariableInfo {
                        name: slot.name.clone(),
                        value: debug::format_value(&value),
                        ty: debug::format_var_type_with_width(slot.ty, slot.int_width)
                            .to_string(),
                        var_ref: 0,
                    });
                }
            }
        }
        result
    }

    /// Read-only access to the global values slice (for retain capture).
    pub fn globals_ref(&self) -> &[Value] {
        &self.globals
    }

    /// Set a global variable by slot index, bypassing force checks.
    /// Used by retain restore at startup before any scan cycle runs.
    pub fn set_global_unchecked(&mut self, slot: u16, value: Value) {
        if (slot as usize) < self.globals.len() {
            self.globals[slot as usize] = value;
        }
    }

    /// Read-only access to the retained locals map (for retain capture).
    pub fn retained_locals_ref(&self) -> &std::collections::HashMap<u16, Vec<Value>> {
        &self.retained_locals
    }

    /// Inject retained locals for a program (used by retain restore).
    pub fn set_retained_locals(&mut self, func_index: u16, locals: Vec<Value>) {
        self.retained_locals.insert(func_index, locals);
    }

    /// Read-only access to the FB instance state map. Used by the DAP
    /// server to build hierarchical variable views.
    pub fn fb_instances_ref(&self) -> &std::collections::HashMap<(u32, u16), Vec<Value>> {
        &self.fb_instances
    }

    /// Public accessor for caller_identity (used by the DAP to compute
    /// instance keys for the current frame's FB instances).
    pub fn caller_identity_pub(&self) -> u32 {
        self.caller_identity()
    }

    /// Call depth (for stepping logic).
    pub fn call_depth(&self) -> usize {
        self.call_stack.len()
    }

    // =========================================================================
    // Core execution loop
    // =========================================================================

    fn execute(&mut self) -> Result<Value, VmError> {
        loop {
            if self.call_stack.is_empty() {
                return Ok(Value::Void);
            }

            let frame = self.call_stack.last().unwrap();
            let func_index = frame.func_index;
            let pc = frame.pc;

            let func = &self.module.functions[func_index as usize];
            if pc >= func.instructions.len() {
                // Fell off the end — implicit return
                let ret_val = Value::Void;
                self.save_retained_locals();
                self.call_stack.pop();
                if let Some(caller) = self.call_stack.last_mut() {
                    if let Some(ret_reg) = caller.return_reg.take() {
                        caller.registers[ret_reg as usize] = ret_val;
                    }
                }
                continue;
            }

            // Debug: check if we should pause before this instruction
            let source_map = &func.source_map;
            let call_depth = self.call_stack.len();
            if let Some(reason) = self.debug.should_pause(func_index, pc, call_depth, source_map) {
                self.debug.mark_paused(reason);
                return Err(VmError::Halt);
            }

            let instr = func.instructions[pc].clone();
            // Advance PC before executing (so jumps can override)
            self.call_stack.last_mut().unwrap().pc += 1;

            // Instruction limit
            self.instruction_count += 1;
            if self.config.max_instructions > 0
                && self.instruction_count > self.config.max_instructions
            {
                return Err(VmError::ExecutionLimit(self.config.max_instructions));
            }

            match instr {
                Instruction::Nop => {}

                Instruction::LoadConst(dst, val) => {
                    self.reg_set(dst, val);
                }
                Instruction::Move(dst, src) => {
                    let val = self.reg_get(src).clone();
                    self.reg_set(dst, val);
                }

                Instruction::LoadLocal(dst, slot) => {
                    // Check if this variable is forced
                    let func = &self.module.functions[func_index as usize];
                    let val = if let Some(name) = func.locals.slots.get(slot as usize).map(|s| &s.name) {
                        if let Some(forced) = self.forced_variables.get(&name.to_uppercase()) {
                            forced.clone()
                        } else {
                            self.local_get(slot).clone()
                        }
                    } else {
                        self.local_get(slot).clone()
                    };
                    self.reg_set(dst, val);
                }
                Instruction::StoreLocal(slot, src) => {
                    let val = self.reg_get(src).clone();
                    self.local_set(slot, val);
                }
                Instruction::LoadGlobal(dst, slot) => {
                    // Check if this global is forced
                    let val = if let Some(name) = self.module.globals.slots.get(slot as usize).map(|s| &s.name) {
                        if let Some(forced) = self.forced_variables.get(&name.to_uppercase()) {
                            forced.clone()
                        } else {
                            self.globals[slot as usize].clone()
                        }
                    } else {
                        self.globals[slot as usize].clone()
                    };
                    self.reg_set(dst, val);
                }
                Instruction::StoreGlobal(slot, src) => {
                    // Forced globals reject program writes too — that's
                    // the whole point of force in a PLC. The forced value
                    // stays put until the user unforces it.
                    if !self.forced_global_slots.contains(&slot) {
                        let val = self.reg_get(src).clone();
                        // Narrow to the slot's declared integer width so
                        // sub-i64 types wrap on overflow per IEC 61131-3.
                        let width = self
                            .module
                            .globals
                            .slots
                            .get(slot as usize)
                            .map(|s| s.int_width)
                            .unwrap_or(IntWidth::None);
                        self.globals[slot as usize] = narrow_value(val, width);
                    }
                }

                // Arithmetic
                Instruction::Add(dst, l, r) => {
                    // Wrapping arithmetic so SINT/INT/DINT overflow rolls
                    // over per IEC 61131-3 (and so debug builds don't
                    // panic on intentional overflow). The store-time
                    // narrow_value call truncates to the slot's declared
                    // width.
                    let result = self.arith_op(l, r, i64::wrapping_add, |a, b| a + b);
                    self.reg_set(dst, result);
                }
                Instruction::Sub(dst, l, r) => {
                    let result = self.arith_op(l, r, i64::wrapping_sub, |a, b| a - b);
                    self.reg_set(dst, result);
                }
                Instruction::Mul(dst, l, r) => {
                    let result = self.arith_op(l, r, i64::wrapping_mul, |a, b| a * b);
                    self.reg_set(dst, result);
                }
                Instruction::Div(dst, l, r) => {
                    let rv = self.reg_get(r);
                    if rv.as_int() == 0 && matches!(rv, Value::Int(_) | Value::UInt(_)) {
                        return Err(VmError::DivisionByZero);
                    }
                    let result = self.arith_op(l, r, |a, b| a / b, |a, b| a / b);
                    self.reg_set(dst, result);
                }
                Instruction::Mod(dst, l, r) => {
                    let rv = self.reg_get(r);
                    if rv.as_int() == 0 {
                        return Err(VmError::DivisionByZero);
                    }
                    let result = Value::Int(self.reg_get(l).as_int() % self.reg_get(r).as_int());
                    self.reg_set(dst, result);
                }
                Instruction::Pow(dst, l, r) => {
                    let result = Value::Real(
                        self.reg_get(l).as_real().powf(self.reg_get(r).as_real()),
                    );
                    self.reg_set(dst, result);
                }
                Instruction::Neg(dst, src) => {
                    let val = self.reg_get(src);
                    let result = match val {
                        Value::Int(i) => Value::Int(-i),
                        Value::Real(r) => Value::Real(-r),
                        _ => Value::Int(-val.as_int()),
                    };
                    self.reg_set(dst, result);
                }

                // Comparison
                Instruction::CmpEq(dst, l, r) => {
                    let result = self.cmp_op(l, r, |a, b| a == b, |a, b| a == b);
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::CmpNe(dst, l, r) => {
                    let result = self.cmp_op(l, r, |a, b| a != b, |a, b| a != b);
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::CmpLt(dst, l, r) => {
                    let result = self.cmp_op(l, r, |a, b| a < b, |a, b| a < b);
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::CmpGt(dst, l, r) => {
                    let result = self.cmp_op(l, r, |a, b| a > b, |a, b| a > b);
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::CmpLe(dst, l, r) => {
                    let result = self.cmp_op(l, r, |a, b| a <= b, |a, b| a <= b);
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::CmpGe(dst, l, r) => {
                    let result = self.cmp_op(l, r, |a, b| a >= b, |a, b| a >= b);
                    self.reg_set(dst, Value::Bool(result));
                }

                // Logic
                Instruction::And(dst, l, r) => {
                    let result = self.reg_get(l).as_bool() && self.reg_get(r).as_bool();
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::Or(dst, l, r) => {
                    let result = self.reg_get(l).as_bool() || self.reg_get(r).as_bool();
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::Xor(dst, l, r) => {
                    let result = self.reg_get(l).as_bool() ^ self.reg_get(r).as_bool();
                    self.reg_set(dst, Value::Bool(result));
                }
                Instruction::Not(dst, src) => {
                    let result = !self.reg_get(src).as_bool();
                    self.reg_set(dst, Value::Bool(result));
                }

                // Math intrinsics
                Instruction::Sqrt(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().sqrt()));
                }
                Instruction::Sin(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().sin()));
                }
                Instruction::Cos(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().cos()));
                }
                Instruction::Tan(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().tan()));
                }
                Instruction::Asin(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().asin()));
                }
                Instruction::Acos(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().acos()));
                }
                Instruction::Atan(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().atan()));
                }
                Instruction::SystemTime(dst) => {
                    self.reg_set(dst, Value::Time(self.elapsed_time_ms));
                }
                Instruction::Ln(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().ln()));
                }
                Instruction::Log(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().log10()));
                }
                Instruction::Exp(dst, src) => {
                    self.reg_set(dst, Value::Real(self.reg_get(src).as_real().exp()));
                }

                // Conversion
                Instruction::ToInt(dst, src) => {
                    let val = Value::Int(self.reg_get(src).as_int());
                    self.reg_set(dst, val);
                }
                Instruction::ToReal(dst, src) => {
                    let val = Value::Real(self.reg_get(src).as_real());
                    self.reg_set(dst, val);
                }
                Instruction::ToBool(dst, src) => {
                    let val = Value::Bool(self.reg_get(src).as_bool());
                    self.reg_set(dst, val);
                }

                // Control flow
                Instruction::Jump(label) => {
                    self.jump_to(label)?;
                }
                Instruction::JumpIf(reg, label) => {
                    if self.reg_get(reg).as_bool() {
                        self.jump_to(label)?;
                    }
                }
                Instruction::JumpIfNot(reg, label) => {
                    if !self.reg_get(reg).as_bool() {
                        self.jump_to(label)?;
                    }
                }

                // Function calls
                Instruction::Call {
                    func_index,
                    dst,
                    args,
                } => {
                    self.call_function(func_index, Some(dst), &args)?;
                }
                Instruction::CallFb {
                    instance_slot,
                    func_index,
                    args,
                } => {
                    self.call_fb(func_index, instance_slot, &args)?;
                }
                Instruction::Ret(reg) => {
                    let ret_val = self.reg_get(reg).clone();
                    self.save_retained_locals();
                    self.call_stack.pop();
                    if let Some(caller) = self.call_stack.last_mut() {
                        if let Some(ret_reg) = caller.return_reg.take() {
                            caller.registers[ret_reg as usize] = ret_val.clone();
                        }
                    }
                    if self.call_stack.is_empty() {
                        return Ok(ret_val);
                    }
                }
                Instruction::RetVoid => {
                    self.save_retained_locals();
                    self.call_stack.pop();
                    if self.call_stack.is_empty() {
                        return Ok(Value::Void);
                    }
                    if let Some(caller) = self.call_stack.last_mut() {
                        caller.return_reg.take();
                    }
                }

                // Partial access (bit/byte/word/dword extraction/insertion)
                Instruction::ExtractBit(dst, src, bit_idx) => {
                    let val = self.reg_get(src).as_int();
                    let bit = (val >> bit_idx) & 1;
                    self.reg_set(dst, Value::Bool(bit != 0));
                }
                Instruction::InsertBit(dst, src, bit_idx, val_reg) => {
                    let mut base = self.reg_get(src).as_int();
                    let bit_val = self.reg_get(val_reg).as_bool();
                    if bit_val {
                        base |= 1 << bit_idx;
                    } else {
                        base &= !(1 << bit_idx);
                    }
                    self.reg_set(dst, Value::Int(base));
                }
                Instruction::ExtractPartial(dst, src, index, size_bits) => {
                    let val = self.reg_get(src).as_int();
                    let shift = (index as i64) * (size_bits as i64);
                    let mask = if size_bits >= 64 { -1i64 } else { (1i64 << size_bits) - 1 };
                    let extracted = (val >> shift) & mask;
                    self.reg_set(dst, Value::Int(extracted));
                }
                Instruction::InsertPartial(dst, src, index, size_bits, val_reg) => {
                    let mut base = self.reg_get(src).as_int();
                    let new_val = self.reg_get(val_reg).as_int();
                    let shift = (index as i64) * (size_bits as i64);
                    let mask = if size_bits >= 64 { -1i64 } else { (1i64 << size_bits) - 1 };
                    base &= !(mask << shift);
                    base |= (new_val & mask) << shift;
                    self.reg_set(dst, Value::Int(base));
                }

                // Array/struct access (simplified)
                Instruction::LoadArray(dst, _slot, _idx) => {
                    self.reg_set(dst, Value::Int(0)); // TODO: implement array storage
                }
                Instruction::StoreArray(_slot, _idx, _val) => {
                    // TODO: implement array storage
                }
                Instruction::LoadField(dst, instance_slot, field_idx) => {
                    // Read a field from an FB instance's retained state
                    let instance_key = (self.caller_identity(), instance_slot);
                    let val = self.fb_instances
                        .get(&instance_key)
                        .and_then(|locals| locals.get(field_idx as usize))
                        .cloned()
                        .unwrap_or(Value::Int(0));
                    self.reg_set(dst, val);
                }
                Instruction::StoreField(instance_slot, field_idx, val_reg) => {
                    // Write a field to an FB/class instance's retained state
                    let instance_key = (self.caller_identity(), instance_slot);
                    let val = self.reg_get(val_reg).clone();
                    let entry = self.fb_instances
                        .entry(instance_key)
                        .or_insert_with(|| {
                            // No state exists yet — create with enough capacity
                            vec![Value::default(); field_idx as usize + 1]
                        });
                    // Extend if needed (rare, but safe)
                    while entry.len() <= field_idx as usize {
                        entry.push(Value::default());
                    }
                    entry[field_idx as usize] = val;
                }

                // Pointer operations
                // Encoding: Ref(scope_tag, slot)
                //   scope_tag 0 = global
                //   scope_tag >= 2 = call stack frame (frame_index = scope_tag - 2)
                //   (tag 1 was previously "global" — now 0; tag 1 reserved for future use)
                Instruction::MakeRefLocal(dst, slot) => {
                    // Encode the current call stack index so the pointer works
                    // correctly even when passed to other functions.
                    let frame_tag = (self.call_stack.len() as u16 - 1) + 2;
                    self.reg_set(dst, Value::Ref(frame_tag, slot));
                }
                Instruction::MakeRefGlobal(dst, slot) => {
                    self.reg_set(dst, Value::Ref(0, slot));
                }
                Instruction::LoadNull(dst) => {
                    self.reg_set(dst, Value::Null);
                }
                Instruction::Deref(dst, ptr_reg) => {
                    let ptr = self.reg_get(ptr_reg).clone();
                    let val = match ptr {
                        Value::Ref(0, slot) => {
                            // Global variable
                            self.globals.get(slot as usize).cloned().unwrap_or(Value::Void)
                        }
                        Value::Ref(tag, slot) if tag >= 2 => {
                            // Local variable in a specific call stack frame
                            let frame_idx = (tag - 2) as usize;
                            if let Some(frame) = self.call_stack.get(frame_idx) {
                                frame.locals.get(slot as usize).cloned().unwrap_or(Value::Int(0))
                            } else {
                                Value::Int(0)
                            }
                        }
                        Value::Ref(1, slot) => {
                            // Legacy: treat tag=1 as global for backward compatibility
                            self.globals.get(slot as usize).cloned().unwrap_or(Value::Void)
                        }
                        Value::Null => Value::Int(0),
                        _ => Value::Int(0),
                    };
                    self.reg_set(dst, val);
                }
                Instruction::DerefStore(ptr_reg, val_reg) => {
                    let ptr = self.reg_get(ptr_reg).clone();
                    let val = self.reg_get(val_reg).clone();
                    match ptr {
                        Value::Ref(0, slot) => {
                            self.globals[slot as usize] = val;
                        }
                        Value::Ref(tag, slot) if tag >= 2 => {
                            let frame_idx = (tag - 2) as usize;
                            if let Some(frame) = self.call_stack.get_mut(frame_idx) {
                                if (slot as usize) < frame.locals.len() {
                                    frame.locals[slot as usize] = val;
                                }
                            }
                        }
                        Value::Ref(1, slot) => {
                            // Legacy: tag=1 as global
                            if (slot as usize) < self.globals.len() {
                                self.globals[slot as usize] = val;
                            }
                        }
                        _ => {} // null deref is a no-op
                    }
                }
                Instruction::CallMethod { instance_slot, class_func_index, func_index, dst, args } => {
                    // Method call for class instances.
                    // Method locals layout: [method_class_vars... | method_vars... | return_var?]
                    // The method's class vars match the method's DEFINING class, which may
                    // be a parent of the actual instance class.
                    let method_func = &self.module.functions[func_index as usize];
                    let class_func = &self.module.functions[class_func_index as usize];
                    let register_count = method_func.register_count as usize;
                    let instance_var_count = class_func.locals.slots.len();

                    // Figure out how many class vars the METHOD expects
                    // (the method was compiled with its defining class's var layout)
                    let method_class_var_count = self.find_method_class_var_count(func_index, class_func_index);

                    // Initialize all method locals to defaults
                    let mut locals: Vec<Value> = method_func
                        .locals
                        .slots
                        .iter()
                        .map(|s| Value::default_for_type(s.ty))
                        .collect();

                    // Get or create instance state
                    let instance_key = (self.caller_identity(), instance_slot);

                    if let Some(saved) = self.fb_instances.get(&instance_key) {
                        // Restore saved state: copy the portion that matches method's class vars
                        let n = method_class_var_count.min(saved.len()).min(locals.len());
                        locals[..n].clone_from_slice(&saved[..n]);
                    } else {
                        // First use: initialize instance via class init code
                        let mut instance_state = vec![Value::default(); instance_var_count];
                        self.init_class_instance(class_func_index, &mut instance_state, instance_var_count);
                        // Copy the matching portion into method locals
                        let n = method_class_var_count.min(instance_state.len()).min(locals.len());
                        locals[..n].clone_from_slice(&instance_state[..n]);
                        // Save the full instance state for future calls
                        self.fb_instances.insert(instance_key, instance_state);
                    }

                    // Apply method arguments (offset past the method's class vars)
                    for (param_slot, arg_reg) in &args {
                        let actual_slot = method_class_var_count as u16 + *param_slot;
                        let val = self.reg_get(*arg_reg).clone();
                        if (actual_slot as usize) < locals.len() {
                            locals[actual_slot as usize] = val;
                        }
                    }

                    if self.call_stack.len() >= self.config.max_call_depth {
                        return Err(VmError::StackOverflow(self.config.max_call_depth));
                    }

                    // Set return_reg on the CALLER frame (like call_function does)
                    if let Some(caller) = self.call_stack.last_mut() {
                        caller.return_reg = Some(dst);
                    }

                    self.call_stack.push(CallFrame {
                        func_index,
                        registers: vec![Value::default(); register_count.max(1)],
                        locals,
                        pc: 0,
                        return_reg: None,
                        instance_slot: Some(instance_slot),
                    });
                    continue;
                }
            }
        }
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn reg_get(&self, reg: Reg) -> &Value {
        &self.call_stack.last().unwrap().registers[reg as usize]
    }

    fn reg_set(&mut self, reg: Reg, val: Value) {
        self.call_stack.last_mut().unwrap().registers[reg as usize] = val;
    }

    fn local_get(&self, slot: u16) -> &Value {
        &self.call_stack.last().unwrap().locals[slot as usize]
    }

    fn local_set(&mut self, slot: u16, val: Value) {
        // Narrow the value to the slot's declared integer width so SINT
        // overflow wraps at -128/127 instead of growing into the i64 range.
        let frame = self.call_stack.last_mut().unwrap();
        let func = &self.module.functions[frame.func_index as usize];
        let width = func
            .locals
            .slots
            .get(slot as usize)
            .map(|s| s.int_width)
            .unwrap_or(IntWidth::None);
        frame.locals[slot as usize] = narrow_value(val, width);
    }

    /// Save local variables of the current frame if it's a PROGRAM POU.
    fn save_retained_locals(&mut self) {
        if let Some(frame) = self.call_stack.last() {
            let func = &self.module.functions[frame.func_index as usize];
            match func.kind {
                st_ir::PouKind::Program => {
                    self.retained_locals
                        .insert(frame.func_index, frame.locals.clone());
                }
                st_ir::PouKind::FunctionBlock => {
                    // Save FB instance state keyed by (caller_identity, instance_slot)
                    if let Some(slot) = frame.instance_slot {
                        if self.call_stack.len() >= 2 {
                            let caller_id = self.caller_identity_at(self.call_stack.len() - 2);
                            self.fb_instances
                                .insert((caller_id, slot), frame.locals.clone());
                        }
                    }
                }
                st_ir::PouKind::Function => {
                    // Functions don't retain state
                }
                st_ir::PouKind::Method => {
                    // Methods write back only their defining class's vars into the instance state.
                    // This preserves vars from sibling/parent classes that this method doesn't see.
                    if let Some(slot) = frame.instance_slot {
                        if self.call_stack.len() >= 2 {
                            let caller_id = self.caller_identity_at(self.call_stack.len() - 2);
                            let instance_key = (caller_id, slot);
                            let method_class_count = self.find_method_class_var_count(
                                frame.func_index,
                                0, // not used in current impl
                            );
                            if let Some(state) = self.fb_instances.get_mut(&instance_key) {
                                // Merge: only overwrite the slots this method owns
                                let n = method_class_count.min(state.len()).min(frame.locals.len());
                                state[..n].clone_from_slice(&frame.locals[..n]);
                            } else {
                                // No existing state — save what we have
                                let save: Vec<Value> = frame.locals[..method_class_count.min(frame.locals.len())].to_vec();
                                self.fb_instances.insert(instance_key, save);
                            }
                        }
                    }
                }
                st_ir::PouKind::Class => {
                    // Classes retain state like FBs
                    if let Some(slot) = frame.instance_slot {
                        if self.call_stack.len() >= 2 {
                            let caller_id = self.caller_identity_at(self.call_stack.len() - 2);
                            self.fb_instances
                                .insert((caller_id, slot), frame.locals.clone());
                        }
                    }
                }
            }
        }
    }

    /// Call a function block instance, loading/saving its persistent state.
    fn call_fb(
        &mut self,
        func_index: u16,
        instance_slot: u16,
        args: &[(u16, Reg)],
    ) -> Result<(), VmError> {
        if self.call_stack.len() >= self.config.max_call_depth {
            return Err(VmError::StackOverflow(self.config.max_call_depth));
        }

        let instance_key = (self.caller_identity(), instance_slot);

        let func = self
            .module
            .functions
            .get(func_index as usize)
            .ok_or(VmError::InvalidFunction(func_index))?;

        let register_count = func.register_count as usize;

        // Load retained instance locals, or create fresh ones
        let expected_len = func.locals.slots.len();
        let mut locals: Vec<Value> = self
            .fb_instances
            .get(&instance_key)
            .cloned()
            .unwrap_or_else(|| {
                func.locals
                    .slots
                    .iter()
                    .map(|s| Value::default_for_type(s.ty))
                    .collect()
            });
        // Ensure locals is at least as long as the function expects
        // (StoreField may have created a shorter state)
        while locals.len() < expected_len {
            let ty = func.locals.slots.get(locals.len())
                .map(|s| s.ty)
                .unwrap_or(VarType::Int);
            locals.push(Value::default_for_type(ty));
        }

        // Apply input arguments on top of retained state
        for &(param_slot, arg_reg) in args {
            let val = self.reg_get(arg_reg).clone();
            if (param_slot as usize) < locals.len() {
                locals[param_slot as usize] = val;
            }
        }

        self.call_stack.push(CallFrame {
            func_index,
            registers: vec![Value::default(); register_count.max(1)],
            locals,
            pc: func.body_start_pc, // skip init code for FB instances too
            return_reg: None,
            instance_slot: Some(instance_slot),
        });

        Ok(())
    }

    /// Compute a unique identity for the current caller frame.
    /// Combines func_index with the caller's own instance_slot to distinguish
    /// nested instances (e.g., class inside different FB instances).
    fn caller_identity(&self) -> u32 {
        if let Some(frame) = self.call_stack.last() {
            let func_id = frame.func_index as u32;
            let inst_id = frame.instance_slot.unwrap_or(0xFFFF) as u32;
            // Combine: upper 16 bits = instance_slot, lower 16 = func_index
            (inst_id << 16) | func_id
        } else {
            0
        }
    }

    /// Same but for a specific stack depth (for the caller of the current frame).
    fn caller_identity_at(&self, stack_depth: usize) -> u32 {
        if let Some(frame) = self.call_stack.get(stack_depth) {
            let func_id = frame.func_index as u32;
            let inst_id = frame.instance_slot.unwrap_or(0xFFFF) as u32;
            (inst_id << 16) | func_id
        } else {
            0
        }
    }

    /// Determine how many class vars a method expects in its locals.
    /// A method compiled for class X has X's full inherited var chain in its locals.
    /// This is determined by finding the method's defining class function.
    fn find_method_class_var_count(&self, method_func_index: u16, instance_class_index: u16) -> usize {
        let method_name = &self.module.functions[method_func_index as usize].name;
        // Method name is "ClassName.MethodName" — extract the class name
        if let Some(dot_pos) = method_name.find('.') {
            let defining_class = &method_name[..dot_pos];
            // Find the defining class's function to get its var count
            if let Some((_, cls_func)) = self.module.find_function(defining_class) {
                return cls_func.locals.slots.len();
            }
        }
        // Fallback: use the instance class's var count
        self.module.functions[instance_class_index as usize].locals.slots.len()
    }

    /// Initialize class instance vars by interpreting the class function's init code.
    /// This runs the LoadConst+StoreLocal pairs that set default values.
    fn init_class_instance(&self, class_func_index: u16, locals: &mut [Value], class_var_count: usize) {
        let class_func = &self.module.functions[class_func_index as usize];
        // Simple interpreter for init code: look for LoadConst+StoreLocal pairs
        let mut registers: Vec<Value> = vec![Value::default(); 256];
        for instr in &class_func.instructions {
            match instr {
                Instruction::LoadConst(dst, val) => {
                    registers[*dst as usize] = val.clone();
                }
                Instruction::StoreLocal(slot, src) => {
                    if (*slot as usize) < class_var_count && (*slot as usize) < locals.len() {
                        locals[*slot as usize] = registers[*src as usize].clone();
                    }
                }
                Instruction::RetVoid | Instruction::Ret(_) => break,
                _ => {}
            }
        }
    }

    fn jump_to(&mut self, label: Label) -> Result<(), VmError> {
        let frame = self.call_stack.last_mut().unwrap();
        let func = &self.module.functions[frame.func_index as usize];
        let target = *func
            .label_positions
            .get(label as usize)
            .ok_or(VmError::InvalidLabel(label))?;
        frame.pc = target;
        Ok(())
    }

    fn call_function(
        &mut self,
        func_index: u16,
        return_reg: Option<Reg>,
        args: &[(u16, Reg)],
    ) -> Result<(), VmError> {
        if self.call_stack.len() >= self.config.max_call_depth {
            return Err(VmError::StackOverflow(self.config.max_call_depth));
        }

        let func = self
            .module
            .functions
            .get(func_index as usize)
            .ok_or(VmError::InvalidFunction(func_index))?;

        let register_count = func.register_count as usize;
        let mut locals: Vec<Value> = func
            .locals
            .slots
            .iter()
            .map(|s| Value::default_for_type(s.ty))
            .collect();

        // Pass arguments: store arg values into callee's local slots
        for &(param_slot, arg_reg) in args {
            let val = self.reg_get(arg_reg).clone();
            if (param_slot as usize) < locals.len() {
                locals[param_slot as usize] = val;
            }
        }

        // Set return_reg on the CALLER frame
        if let Some(ret_reg) = return_reg {
            if let Some(caller) = self.call_stack.last_mut() {
                caller.return_reg = Some(ret_reg);
            }
        }

        self.call_stack.push(CallFrame {
            func_index,
            registers: vec![Value::default(); register_count.max(1)],
            locals,
            pc: 0,
            return_reg: None,
            instance_slot: None,
        });

        Ok(())
    }

    fn arith_op(
        &self,
        l: Reg,
        r: Reg,
        int_op: impl Fn(i64, i64) -> i64,
        real_op: impl Fn(f64, f64) -> f64,
    ) -> Value {
        let lv = self.reg_get(l);
        let rv = self.reg_get(r);
        match (lv, rv) {
            (Value::Real(_), _) | (_, Value::Real(_)) => {
                Value::Real(real_op(lv.as_real(), rv.as_real()))
            }
            (Value::Time(_), _) | (_, Value::Time(_)) => {
                Value::Time(int_op(lv.as_int(), rv.as_int()))
            }
            _ => Value::Int(int_op(lv.as_int(), rv.as_int())),
        }
    }

    fn cmp_op(
        &self,
        l: Reg,
        r: Reg,
        int_cmp: impl Fn(i64, i64) -> bool,
        real_cmp: impl Fn(f64, f64) -> bool,
    ) -> bool {
        let lv = self.reg_get(l);
        let rv = self.reg_get(r);
        match (lv, rv) {
            // Pointer/Null comparisons: only equality matters
            (Value::Null, Value::Null) => int_cmp(0, 0),
            (Value::Ref(..), Value::Null) | (Value::Null, Value::Ref(..)) => {
                // Ref vs Null: they are NOT equal
                int_cmp(1, 0) // 1 != 0 for NE, 1 == 0 false for EQ
            }
            (Value::Ref(t1, s1), Value::Ref(t2, s2)) => {
                // Two refs: compare by identity (same scope + slot)
                let same = t1 == t2 && s1 == s2;
                if same { int_cmp(0, 0) } else { int_cmp(1, 0) }
            }
            (Value::Real(_), _) | (_, Value::Real(_)) => {
                real_cmp(lv.as_real(), rv.as_real())
            }
            _ => int_cmp(lv.as_int(), rv.as_int()),
        }
    }
}

/// Narrow an integer value to its declared bit width using two's complement
/// wrapping. Called at every store site (StoreLocal, StoreGlobal,
/// set_global, set_global_by_slot, force_variable) so a SINT cycle counter
/// wraps at 127→-128 instead of growing into the i64 range.
///
/// Non-integer values and `IntWidth::None` slots passthrough unchanged, so
/// this is safe to call unconditionally on every store.
fn narrow_value(val: Value, width: IntWidth) -> Value {
    match width {
        IntWidth::None => val,
        // 64-bit widths don't narrow but DO normalize the Value variant
        // so a slot declared as ULINT always holds Value::UInt (not
        // Value::Int from a signed-literal source).
        IntWidth::I64 => match val {
            Value::Int(_) => val,
            Value::UInt(u) => Value::Int(u as i64),
            other => other,
        },
        IntWidth::U64 => match val {
            Value::UInt(_) => val,
            Value::Int(i) => Value::UInt(i as u64),
            other => other,
        },
        IntWidth::I8 => match val {
            Value::Int(i) => Value::Int(i as i8 as i64),
            Value::UInt(u) => Value::Int(u as i8 as i64),
            other => other,
        },
        IntWidth::U8 => match val {
            Value::Int(i) => Value::UInt(i as u8 as u64),
            Value::UInt(u) => Value::UInt(u as u8 as u64),
            other => other,
        },
        IntWidth::I16 => match val {
            Value::Int(i) => Value::Int(i as i16 as i64),
            Value::UInt(u) => Value::Int(u as i16 as i64),
            other => other,
        },
        IntWidth::U16 => match val {
            Value::Int(i) => Value::UInt(i as u16 as u64),
            Value::UInt(u) => Value::UInt(u as u16 as u64),
            other => other,
        },
        IntWidth::I32 => match val {
            Value::Int(i) => Value::Int(i as i32 as i64),
            Value::UInt(u) => Value::Int(u as i32 as i64),
            other => other,
        },
        IntWidth::U32 => match val {
            Value::Int(i) => Value::UInt(i as u32 as u64),
            Value::UInt(u) => Value::UInt(u as u32 as u64),
            other => other,
        },
    }
}
