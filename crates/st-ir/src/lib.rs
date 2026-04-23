//! Intermediate representation and bytecode definitions for the PLC VM.
//!
//! Register-based IR with typed operations. Each register holds a [`Value`].
//! Programs are compiled into [`Module`]s containing [`Function`] definitions,
//! each with a sequence of [`Instruction`]s.

use serde::{Deserialize, Serialize};

/// A register index (u16 allows 65536 registers per function — more than enough).
pub type Reg = u16;

/// A label index for jump targets within a function.
pub type Label = u32;

/// A compiled module — the output of the compiler for one source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// All compiled functions/FBs/programs in this module.
    pub functions: Vec<Function>,
    /// Global variable storage layout.
    pub globals: MemoryLayout,
    /// User-defined type definitions (for runtime struct/array construction).
    pub type_defs: Vec<TypeDef>,
    /// Indices into `functions` that are native (Rust-backed) FBs.
    /// These have empty instruction bodies — the VM dispatches to
    /// `NativeFb::execute()` instead of interpreting bytecode.
    #[serde(default)]
    pub native_fb_indices: Vec<u16>,
}

/// A compiled function, function block, or program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub kind: PouKind,
    /// Number of registers used by this function.
    pub register_count: u16,
    /// The instruction sequence.
    pub instructions: Vec<Instruction>,
    /// Label → instruction index mapping.
    pub label_positions: Vec<usize>,
    /// Variable layout for this function's local frame.
    pub locals: MemoryLayout,
    /// Source map: instruction index → source byte offset.
    pub source_map: Vec<SourceLocation>,
    /// PC where the body starts (after VAR initialization code).
    /// Used to skip init when re-running with retained locals.
    pub body_start_pc: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PouKind {
    Function,
    FunctionBlock,
    Program,
    Class,
    Method,
}

/// Memory layout for a set of variables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryLayout {
    pub slots: Vec<VarSlot>,
}

/// A single variable's location in a memory frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarSlot {
    pub name: String,
    pub ty: VarType,
    pub offset: usize,
    pub size: usize,
    pub retain: bool,
    #[serde(default)]
    pub persistent: bool,
    /// Original integer width / signedness from the source declaration.
    /// Used by the VM at store time to wrap values according to two's
    /// complement semantics — so a SINT cycle counter wraps at 127→-128
    /// instead of growing into the i64 range. `IntWidth::None` for
    /// non-integer slots, including LINT/ULINT (which already match the
    /// VM's native width).
    #[serde(default)]
    pub int_width: IntWidth,
}

/// Original integer width + signedness for a variable slot. The VM uses
/// this to narrow stored values to the declared bit width via two's
/// complement wrapping, matching the behavior of every IEC 61131-3 PLC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum IntWidth {
    /// Not an integer (or no narrowing needed — LINT/ULINT match the
    /// VM's native i64/u64 width).
    #[default]
    None,
    /// 8-bit signed (SINT, range -128..127).
    I8,
    /// 8-bit unsigned (USINT, BYTE, range 0..255).
    U8,
    /// 16-bit signed (INT, range -32768..32767).
    I16,
    /// 16-bit unsigned (UINT, WORD, range 0..65535).
    U16,
    /// 32-bit signed (DINT).
    I32,
    /// 32-bit unsigned (UDINT, DWORD).
    U32,
    /// 64-bit signed (LINT) — passthrough, no narrowing needed.
    I64,
    /// 64-bit unsigned (ULINT, LWORD) — passthrough.
    U64,
}

impl IntWidth {
    /// Display name for this width as a PLC type, e.g. "SINT" / "UDINT".
    pub fn display_name(self) -> Option<&'static str> {
        match self {
            IntWidth::None => None,
            IntWidth::I8 => Some("SINT"),
            IntWidth::U8 => Some("USINT"),
            IntWidth::I16 => Some("INT"),
            IntWidth::U16 => Some("UINT"),
            IntWidth::I32 => Some("DINT"),
            IntWidth::U32 => Some("UDINT"),
            IntWidth::I64 => Some("LINT"),
            IntWidth::U64 => Some("ULINT"),
        }
    }
}

/// Runtime type tag for variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VarType {
    Bool,
    Int,    // 64-bit signed (covers SINT..LINT)
    UInt,   // 64-bit unsigned (covers USINT..ULINT)
    Real,   // 64-bit float (covers REAL, LREAL)
    String, // heap-allocated
    Time,   // milliseconds as i64
    FbInstance(u16),    // index into Module::functions
    ClassInstance(u16), // index into Module::functions (the class)
    Struct(u16),        // index into Module::type_defs
    Array(u16),         // index into Module::type_defs (TypeDef::Array)
    Ref,                // REF_TO pointer
}

impl VarType {
    pub fn size(&self) -> usize {
        match self {
            VarType::Bool => 1,
            VarType::Int | VarType::UInt | VarType::Real | VarType::Time => 8,
            VarType::String => 24, // ptr + len + capacity
            VarType::FbInstance(_) => 0, // size determined by the FB's MemoryLayout
            VarType::ClassInstance(_) => 0, // size determined by the class's MemoryLayout
            VarType::Struct(_) => 0, // fields stored externally (like FB instances)
            VarType::Array(_) => 0,  // elements stored externally (like FB instances)
            VarType::Ref => 4, // scope_tag + slot_index
        }
    }
}

/// A runtime value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Real(f64),
    String(String),
    Time(i64),     // milliseconds
    /// A reference (pointer) to a variable: (scope_tag, slot_index).
    /// scope_tag: 0 = local, 1 = global, 2+ = FB instance.
    Ref(u16, u16),
    /// Null pointer.
    Null,
    Void,
}

impl Value {
    pub fn as_bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::UInt(u) => *u != 0,
            Value::Real(r) => *r != 0.0,
            Value::Time(ms) => *ms != 0,
            _ => false,
        }
    }

    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::UInt(u) => *u as i64,
            Value::Bool(b) => *b as i64,
            Value::Real(r) => *r as i64,
            Value::Time(ms) => *ms,
            _ => 0,
        }
    }

    pub fn as_real(&self) -> f64 {
        match self {
            Value::Real(r) => *r,
            Value::Int(i) => *i as f64,
            Value::UInt(u) => *u as f64,
            Value::Time(ms) => *ms as f64,
            _ => 0.0,
        }
    }

    pub fn as_time(&self) -> i64 {
        match self {
            Value::Time(ms) => *ms,
            Value::Int(i) => *i,
            Value::UInt(u) => *u as i64,
            Value::Real(r) => *r as i64,
            Value::Bool(b) => *b as i64,
            _ => 0,
        }
    }

    pub fn as_uint(&self) -> u64 {
        match self {
            Value::UInt(u) => *u,
            Value::Int(i) => *i as u64,
            Value::Bool(b) => *b as u64,
            Value::Real(r) => *r as u64,
            Value::Time(ms) => *ms as u64,
            _ => 0,
        }
    }

    pub fn default_for_type(ty: VarType) -> Value {
        match ty {
            VarType::Bool => Value::Bool(false),
            VarType::Int => Value::Int(0),
            VarType::UInt => Value::UInt(0),
            VarType::Real => Value::Real(0.0),
            VarType::String => Value::String(String::new()),
            VarType::Time => Value::Time(0),
            VarType::FbInstance(_) => Value::Void,
            VarType::ClassInstance(_) => Value::Void,
            VarType::Struct(_) => Value::Void,
            VarType::Array(_) => Value::Void,
            VarType::Ref => Value::Null,
        }
    }
}

impl Default for Value {
    fn default() -> Self {
        Value::Int(0)
    }
}

/// A single bytecode instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instruction {
    /// No operation.
    Nop,

    // ── Register operations ──────────────────────────────────────────
    /// Load a constant value into a register.
    LoadConst(Reg, Value),
    /// Copy value from one register to another.
    Move(Reg, Reg), // dst, src

    // ── Variable access ──────────────────────────────────────────────
    /// Load a local variable into a register.
    LoadLocal(Reg, u16),  // dst, slot_index
    /// Store a register into a local variable.
    StoreLocal(u16, Reg), // slot_index, src
    /// Load a global variable into a register.
    LoadGlobal(Reg, u16),
    /// Store a register into a global variable.
    StoreGlobal(u16, Reg),

    // ── Arithmetic (operate on registers, result in dst) ─────────────
    /// dst = left + right
    Add(Reg, Reg, Reg),
    /// dst = left - right
    Sub(Reg, Reg, Reg),
    /// dst = left * right
    Mul(Reg, Reg, Reg),
    /// dst = left / right
    Div(Reg, Reg, Reg),
    /// dst = left % right
    Mod(Reg, Reg, Reg),
    /// dst = left ** right (power, result is f64)
    Pow(Reg, Reg, Reg),
    /// dst = -src
    Neg(Reg, Reg),

    // ── Comparison (result is Bool) ──────────────────────────────────
    CmpEq(Reg, Reg, Reg),
    CmpNe(Reg, Reg, Reg),
    CmpLt(Reg, Reg, Reg),
    CmpGt(Reg, Reg, Reg),
    CmpLe(Reg, Reg, Reg),
    CmpGe(Reg, Reg, Reg),

    // ── Logical / bitwise ────────────────────────────────────────────
    And(Reg, Reg, Reg),
    Or(Reg, Reg, Reg),
    Xor(Reg, Reg, Reg),
    Not(Reg, Reg),

    // ── Math intrinsics ────────────────────────────────────────────────
    /// dst = sqrt(src)
    Sqrt(Reg, Reg),
    /// dst = sin(src)
    Sin(Reg, Reg),
    /// dst = cos(src)
    Cos(Reg, Reg),
    /// dst = tan(src)
    Tan(Reg, Reg),
    /// dst = asin(src)
    Asin(Reg, Reg),
    /// dst = acos(src)
    Acos(Reg, Reg),
    /// dst = atan(src)
    Atan(Reg, Reg),
    /// dst = current elapsed time in milliseconds (TIME value)
    SystemTime(Reg),
    /// dst = ln(src)
    Ln(Reg, Reg),
    /// dst = log(src) (base 10)
    Log(Reg, Reg),
    /// dst = exp(src)
    Exp(Reg, Reg),

    // ── Type conversion ──────────────────────────────────────────────
    /// Convert register value to int.
    ToInt(Reg, Reg),
    /// Convert register value to real.
    ToReal(Reg, Reg),
    /// Convert register value to bool.
    ToBool(Reg, Reg),
    /// Convert register value to time (milliseconds).
    ToTime(Reg, Reg),
    /// Convert register value to TOD (wraps to 0..86_399_999 ms).
    ToTod(Reg, Reg),
    /// Extract date from DT: dst = (src / 86400000) * 86400000.
    DtExtractDate(Reg, Reg),
    /// Extract time-of-day from DT: dst = src % 86400000.
    DtExtractTod(Reg, Reg),
    /// Day of week from DATE (ms since epoch): 0=Sunday..6=Saturday.
    DayOfWeek(Reg, Reg),

    // ── Control flow ─────────────────────────────────────────────────
    /// Unconditional jump.
    Jump(Label),
    /// Jump if register is true.
    JumpIf(Reg, Label),
    /// Jump if register is false.
    JumpIfNot(Reg, Label),

    // ── Function calls ───────────────────────────────────────────────
    /// Call a function: func_index, dst register (for return value),
    /// args as (param_slot, src_register) pairs.
    Call {
        func_index: u16,
        dst: Reg,
        args: Vec<(u16, Reg)>,
    },
    /// Call an FB instance: instance_slot, func_index, args.
    CallFb {
        instance_slot: u16,
        func_index: u16,
        args: Vec<(u16, Reg)>,
    },
    /// Return from function (return value in register).
    Ret(Reg),
    /// Call a class method: instance_slot, class func_index, method func_index, args.
    /// The class_func_index identifies the class for instance state management.
    /// The method func_index identifies the compiled method body.
    /// Method locals layout: [class_vars... | method_vars... | return_var?]
    CallMethod {
        instance_slot: u16,
        class_func_index: u16,
        func_index: u16,
        dst: Reg,
        args: Vec<(u16, Reg)>,
    },
    /// Return void (for programs / FBs).
    RetVoid,

    // ── Pointer operations ────────────────────────────────────────────
    /// Take a reference to a local variable: dst = REF(local_slot).
    MakeRefLocal(Reg, u16),
    /// Take a reference to a global variable: dst = REF(global_slot).
    MakeRefGlobal(Reg, u16),
    /// Dereference a pointer (read): dst = ptr^.
    Deref(Reg, Reg),
    /// Dereference a pointer (write): ptr^ := value.
    DerefStore(Reg, Reg),
    /// Load NULL into a register.
    LoadNull(Reg),

    // ── Partial access (bit/byte/word/dword extraction) ────────────────
    /// Extract a bit from a value: dst = (src >> bit_index) & 1.
    /// Result is Bool.
    ExtractBit(Reg, Reg, u8),
    /// Insert a bit into a value: dst = src with bit_index set to val.
    InsertBit(Reg, Reg, u8, Reg),
    /// Extract a byte/word/dword from a value: dst = (src >> (index*size)) & mask.
    /// size_bits: 8=byte, 16=word, 32=dword, 64=lword.
    ExtractPartial(Reg, Reg, u8, u8),  // dst, src, index, size_bits
    /// Insert a byte/word/dword into a value: dst = src with partial replaced.
    InsertPartial(Reg, Reg, u8, u8, Reg),  // dst, src, index, size_bits, val

    // ── Array / struct access ────────────────────────────────────────
    /// Load from array: dst, base_slot, index_register.
    LoadArray(Reg, u16, Reg),
    /// Store to array: base_slot, index_register, value_register.
    StoreArray(u16, Reg, Reg),
    /// Load struct field: dst, base_slot, field_offset.
    LoadField(Reg, u16, u16),
    /// Store struct field: base_slot, field_offset, value_register.
    StoreField(u16, u16, Reg),

    /// Load from array field in FB instance: dst, instance_slot, field_base_offset, index_register.
    /// Accesses `fb_instances[instance][base_offset + index]`.
    LoadFieldIndex(Reg, u16, u16, Reg),
    /// Store to array field in FB instance: instance_slot, field_base_offset, index_register, value_register.
    /// Accesses `fb_instances[instance][base_offset + index]`.
    StoreFieldIndex(u16, u16, Reg, Reg),
}

/// Source location for debugger mapping.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SourceLocation {
    pub byte_offset: usize,
    pub byte_end: usize,
}

/// User-defined type definition for runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDef {
    Struct {
        name: String,
        fields: Vec<VarSlot>,
    },
    Enum {
        name: String,
        variants: Vec<(String, i64)>,
    },
    Array {
        element_type: VarType,
        dimensions: Vec<(i64, i64)>,
    },
}

impl MemoryLayout {
    pub fn find_slot(&self, name: &str) -> Option<(u16, &VarSlot)> {
        self.slots
            .iter()
            .enumerate()
            .find(|(_, s)| s.name.eq_ignore_ascii_case(name))
            .map(|(i, s)| (i as u16, s))
    }

    /// Check if this layout has any array fields that expand inline.
    /// When true, slot indices don't map 1:1 to Vec<Value> indices.
    pub fn has_expanded_arrays(&self) -> bool {
        self.slots.iter().any(|s| matches!(s.ty, VarType::Array(_)) && s.size > 1)
    }

    /// Compute the expanded Vec<Value> index for slot `slot_idx`.
    ///
    /// For native FB layouts with array fields, the offset is stored as
    /// Value-count in `VarSlot.offset`. For regular layouts or when no
    /// arrays are present, this returns `slot_idx` unchanged.
    pub fn expanded_index(&self, slot_idx: usize) -> usize {
        if !self.has_expanded_arrays() {
            return slot_idx;
        }
        self.slots.get(slot_idx).map_or(slot_idx, |s| s.offset)
    }

    /// Total expanded Vec<Value> size (accounting for inline array elements).
    /// For regular layouts, this equals `slots.len()`.
    pub fn expanded_len(&self) -> usize {
        if !self.has_expanded_arrays() {
            return self.slots.len();
        }
        self.slots.iter().map(|s| {
            if matches!(s.ty, VarType::Array(_)) { s.size } else { 1 }
        }).sum()
    }

    pub fn total_size(&self) -> usize {
        self.slots.iter().map(|s| s.offset + s.size).max().unwrap_or(0)
    }
}

impl Module {
    pub fn find_function(&self, name: &str) -> Option<(u16, &Function)> {
        self.functions
            .iter()
            .enumerate()
            .find(|(_, f)| f.name.eq_ignore_ascii_case(name))
            .map(|(i, f)| (i as u16, f))
    }
}
