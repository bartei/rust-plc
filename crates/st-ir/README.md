# st-ir

Intermediate representation and bytecode instruction set for the PLC virtual machine.

## Purpose

Defines the register-based IR that the compiler emits and the VM executes. All values are stored in 64-bit registers. This crate is the contract between the compiler (`st-compiler`) and the runtime (`st-engine`) — they share no other types.

## Public API

### Core Types

```rust
use st_ir::*;

// A compiled program
let module: Module;          // functions + globals + type definitions
let func: &Function;         // instructions + register count + locals + source map
let slot: &VarSlot;          // variable metadata: name, type, offset, size

// Runtime values
let val = Value::Int(42);
let val = Value::Real(3.14);
let val = Value::Bool(true);
let val = Value::String("hello".to_string());
```

- `Module` — Compiled module containing `functions: Vec<Function>`, `globals: MemoryLayout`, `type_defs: Vec<TypeDef>`
- `Function` — Compiled POU with `instructions: Vec<Instruction>`, register count, locals layout, source map entries
- `MemoryLayout` — Ordered list of `VarSlot` with `find_slot(name) -> Option<(u16, &VarSlot)>`
- `VarSlot` — Variable metadata: name, `VarType`, offset, size, retain/persistent flags, `IntWidth`
- `Value` — Runtime value enum: `Bool(bool)`, `Int(i64)`, `UInt(u64)`, `Real(f64)`, `String(String)`, `Time(i64)`, `Ref(u16, u16)`, `Null`, `Void`
- `VarType` — Type classifier: `Bool`, `Int`, `UInt`, `Real`, `String`, `Time`, `FbInstance`, `ClassInstance`, `Struct`, `Ref`
- `IntWidth` — IEC integer width for overflow wrapping: `I8`, `U8`, `I16`, `U16`, `I32`, `U32`, `I64`, `U64`, `None`

### Instruction Set (80+ opcodes)

| Category | Instructions |
|----------|-------------|
| Load/Store | `LoadConst`, `Move`, `LoadLocal`, `StoreLocal`, `LoadGlobal`, `StoreGlobal` |
| Arithmetic | `Add`, `Sub`, `Mul`, `Div`, `Mod`, `Pow`, `Neg` |
| Comparison | `CmpEq`, `CmpNe`, `CmpLt`, `CmpGt`, `CmpLe`, `CmpGe` |
| Logical | `And`, `Or`, `Xor`, `Not` |
| Math | `Sqrt`, `Sin`, `Cos`, `Tan`, `Asin`, `Acos`, `Atan`, `Ln`, `Log`, `Exp`, `SystemTime` |
| Type conversion | `ToInt`, `ToReal`, `ToBool` |
| Control flow | `Jump`, `JumpIf`, `JumpIfNot` |
| Functions | `Call`, `CallFb`, `CallMethod`, `Ret`, `RetVoid` |
| Pointers | `MakeRefLocal`, `MakeRefGlobal`, `Deref`, `DerefStore` |
| Partial access | `ExtractBit`, `InsertBit`, `ExtractPartial`, `InsertPartial` |
| Arrays/Structs | `LoadArray`, `StoreArray`, `LoadField`, `StoreField` |

### Source Mapping

Each instruction can have an associated `SourceLocation` (byte offset range) for debugger line mapping. The `Function.source_map` maps instruction indices to source positions.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `serde` | Serialization for bytecode persistence |

## Functional Description

The IR is designed for:
- **Register-based execution** — No operand stack; all operations use explicit register operands
- **IEC 61131-3 compliance** — Integer overflow wrapping per declared type width (SINT wraps at 8-bit, INT at 16-bit, etc.)
- **Debuggability** — Source locations on every instruction enable line-level breakpoints and stepping
- **Online change** — Module structure supports hot-reload with variable migration
