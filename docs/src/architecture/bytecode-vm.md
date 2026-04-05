# Bytecode VM

The runtime virtual machine lives in `crates/st-runtime`. It has two
main components: the **VM** (`vm.rs`) that executes bytecode, and the
**Engine** (`engine.rs`) that drives the PLC scan-cycle loop.

## Register-Based Architecture

The VM uses a register-based IR rather than a stack-based one. Each
function call allocates a flat array of registers (`Vec<Value>`) sized
to `Function::register_count`. The compiler assigns temporaries to
registers during code generation.

## Value Types

Every register and variable slot holds a `Value`:

```rust
pub enum Value {
    Bool(bool),
    Int(i64),       // covers SINT through LINT
    UInt(u64),      // covers USINT through ULINT
    Real(f64),      // covers REAL and LREAL
    String(String),
    Time(i64),      // nanoseconds
    Void,
}
```

All IEC integer widths widen to `i64`/`u64`; all floats become `f64`.
This keeps the instruction set small -- one `Add`, not eight
width-specific variants.

## Call Frames

Each function invocation pushes a `CallFrame`:

```rust
struct CallFrame {
    func_index: u16,
    registers: Vec<Value>,     // sized to Function::register_count
    locals: Vec<Value>,        // one per VarSlot in the function's MemoryLayout
    pc: usize,                 // program counter (instruction index)
    return_reg: Option<Reg>,   // where to store return value in the caller
}
```

Registers are per-frame and never shared between frames. Local variables
are separate from registers: locals correspond to declared `VAR` slots,
while registers hold intermediate expression results.

## Variable Storage

| Storage | Instructions | Lifetime |
|---|---|---|
| **Locals** (`CallFrame::locals`) | `LoadLocal` / `StoreLocal` | One function invocation |
| **Globals** (`Vm::globals`) | `LoadGlobal` / `StoreGlobal` | Entire VM lifetime (persists across scan cycles) |

Slots are addressed by `u16` indices into the corresponding
`MemoryLayout`.

## Fetch-Decode-Execute Loop

The core loop in `Vm::execute()`:

1. If the call stack is empty, return `Value::Void`.
2. Read the current frame's `pc` and `func_index`.
3. If `pc >= instructions.len()`, perform an implicit return (pop frame).
4. Clone the instruction at `pc` and advance `pc` by one.
5. Increment `instruction_count`; check against `max_instructions`.
6. Match on the `Instruction` variant and execute it.

The PC advances **before** execution so jump instructions simply
overwrite `frame.pc` without off-by-one issues. Instructions are cloned
out of the function vector to avoid borrow conflicts with the mutable
call stack.

## Arithmetic with Int/Real Dispatch

Binary arithmetic dispatches on operand types at runtime:

```rust
fn arith_op(&self, l: Reg, r: Reg,
            int_op: impl Fn(i64, i64) -> i64,
            real_op: impl Fn(f64, f64) -> f64) -> Value {
    match (lv, rv) {
        (Value::Real(_), _) | (_, Value::Real(_)) =>
            Value::Real(real_op(lv.as_real(), rv.as_real())),
        _ =>
            Value::Int(int_op(lv.as_int(), rv.as_int())),
    }
}
```

If **either** operand is `Real`, both promote to `f64`. Otherwise the
operation stays in `i64`. Comparisons follow the same pattern through
`cmp_op()`. Division by zero on integer operands returns
`VmError::DivisionByZero`.

## Control Flow via Labels

Labels are `u32` indices into `Function::label_positions`, which maps
each label to an instruction index. The compiler allocates labels with
`alloc_label()` and resolves them with `place_label()`.

Three jump instructions exist:

- `Jump(label)` -- unconditional.
- `JumpIf(reg, label)` -- jump when register is truthy.
- `JumpIfNot(reg, label)` -- jump when register is falsy.

A WHILE loop compiles to:

```
  place_label(loop_start)
  <condition -> reg>
  JumpIfNot(reg, exit_label)
  <body>
  Jump(loop_start)
  place_label(exit_label)
```

FOR and REPEAT loops follow analogous patterns.

## Function Calls

`Call { func_index, dst, args }` performs:

1. Check depth against `max_call_depth` (default 256).
2. Allocate a new `CallFrame` with default-initialised locals.
3. Copy arguments: each `(param_slot, arg_reg)` pair writes the
   caller's register into the callee's local slot.
4. Set `return_reg` on the **caller's** frame.
5. Push the new frame; execution continues in the callee.

`Ret(reg)` pops the frame and writes the value into the caller's
`return_reg`. `RetVoid` pops without writing (used by PROGRAMs and
FUNCTION_BLOCKs). `CallFb` is the variant for function block instances,
carrying an additional `instance_slot`.

## Safety Limits

| Limit | Default | Error |
|---|---|---|
| `max_call_depth` | 256 | `VmError::StackOverflow` |
| `max_instructions` | 10,000,000 | `VmError::ExecutionLimit` |

Division by zero produces `VmError::DivisionByZero`. Invalid function
or label indices produce `VmError::InvalidFunction` and
`VmError::InvalidLabel`.

## Scan Cycle Engine

The `Engine` (`engine.rs`) wraps a `Vm` and drives it cyclically:

```rust
pub fn run_one_cycle(&mut self) -> Result<Duration, VmError> {
    self.vm.reset_instruction_count();
    self.vm.scan_cycle(&self.program_name)?;
    // check watchdog, update stats
}
```

### CycleStats

```rust
pub struct CycleStats {
    pub cycle_count: u64,
    pub last_cycle_time: Duration,
    pub min_cycle_time: Duration,
    pub max_cycle_time: Duration,
    pub total_time: Duration,
}
```

`avg_cycle_time()` returns `total_time / cycle_count`.

### Watchdog

If `EngineConfig::watchdog_timeout` is set and a single cycle exceeds
that duration, the engine aborts with `VmError::ExecutionLimit`.

### Configuration

```rust
pub struct EngineConfig {
    pub cycle_time: Option<Duration>,      // None = fast as possible
    pub max_cycles: u64,                   // 0 = unlimited
    pub vm_config: VmConfig,
    pub watchdog_timeout: Option<Duration>,
}
```

### Variable Access Between Cycles

```rust
engine.vm().get_global("counter")                       // read
engine.vm_mut().set_global("counter", Value::Int(0))    // write
```

This is the foundation for `st-monitor` (live variable streaming) and
`st-dap` (debug adapter protocol).
