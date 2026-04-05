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

    /// Get a global variable value by name.
    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.module
            .globals
            .find_slot(name)
            .map(|(i, _)| &self.globals[i as usize])
    }

    /// Set a global variable value by name.
    pub fn set_global(&mut self, name: &str, value: Value) {
        if let Some((i, _)) = self.module.globals.find_slot(name) {
            self.globals[i as usize] = value;
        }
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

    /// Force a variable to a specific value. The forced value overrides
    /// the runtime value whenever the variable is read.
    pub fn force_variable(&mut self, name: &str, value: Value) {
        self.forced_variables.insert(name.to_uppercase(), value);
    }

    /// Remove a force on a variable.
    pub fn unforce_variable(&mut self, name: &str) {
        self.forced_variables.remove(&name.to_uppercase());
    }

    /// Get all currently forced variables.
    pub fn forced_variables(&self) -> &std::collections::HashMap<String, Value> {
        &self.forced_variables
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
                    ty: debug::format_var_type(slot.ty).to_string(),
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
                    ty: debug::format_var_type(slot.ty).to_string(),
                    var_ref: 0,
                }
            })
            .collect()
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
                    let val = self.reg_get(src).clone();
                    self.globals[slot as usize] = val;
                }

                // Arithmetic
                Instruction::Add(dst, l, r) => {
                    let result = self.arith_op(l, r, |a, b| a + b, |a, b| a + b);
                    self.reg_set(dst, result);
                }
                Instruction::Sub(dst, l, r) => {
                    let result = self.arith_op(l, r, |a, b| a - b, |a, b| a - b);
                    self.reg_set(dst, result);
                }
                Instruction::Mul(dst, l, r) => {
                    let result = self.arith_op(l, r, |a, b| a * b, |a, b| a * b);
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
                    instance_slot: _,
                    func_index,
                    args,
                } => {
                    self.call_function(func_index, None, &args)?;
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

                // Array/struct access (simplified)
                Instruction::LoadArray(dst, _slot, _idx) => {
                    self.reg_set(dst, Value::Int(0)); // TODO: implement array storage
                }
                Instruction::StoreArray(_slot, _idx, _val) => {
                    // TODO: implement array storage
                }
                Instruction::LoadField(dst, _slot, _offset) => {
                    self.reg_set(dst, Value::Int(0)); // TODO: implement struct storage
                }
                Instruction::StoreField(_slot, _offset, _val) => {
                    // TODO: implement struct storage
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
        self.call_stack.last_mut().unwrap().locals[slot as usize] = val;
    }

    /// Save local variables of the current frame if it's a PROGRAM POU.
    fn save_retained_locals(&mut self) {
        if let Some(frame) = self.call_stack.last() {
            let func = &self.module.functions[frame.func_index as usize];
            if func.kind == st_ir::PouKind::Program {
                self.retained_locals
                    .insert(frame.func_index, frame.locals.clone());
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
            (Value::Real(_), _) | (_, Value::Real(_)) => {
                real_cmp(lv.as_real(), rv.as_real())
            }
            _ => int_cmp(lv.as_int(), rv.as_int()),
        }
    }
}
