# Compiler Pipeline

This chapter describes the five stages that transform IEC 61131-3 Structured
Text source code into executable bytecode.

## Stage 1: Tree-Sitter Parsing

**Crate:** `st-grammar` (`crates/st-grammar/src/lib.rs`)

The grammar crate wraps a tree-sitter parser generated from a custom Structured
Text grammar definition. Key properties:

- **Incremental** -- The LSP passes the previous tree into
  `parser.parse(source, Some(&old_tree))` so only the edited region is
  re-parsed. This keeps keystroke latency low.
- **Error-recovering** -- Syntax errors produce `ERROR` nodes in the CST but
  do not prevent the rest of the tree from being built. The test
  `test_error_recovery` in `st-grammar` validates this: a broken `x := ;`
  statement still yields a valid `source_file` root.
- **70+ named node kinds** -- Defined as string constants in
  `st_grammar::kind` (e.g. `PROGRAM_DECLARATION`, `IF_STATEMENT`,
  `ADDITIVE_EXPRESSION`). These are the glue between the C tree-sitter parser
  and the Rust lowering code.

```rust
// crates/st-grammar/src/lib.rs
pub fn language() -> Language {
    unsafe { Language::from_raw(tree_sitter_structured_text()) }
}
```

## Stage 2: CST-to-AST Lowering

**Crate:** `st-syntax` (`crates/st-syntax/src/lower.rs`)

The `lower()` function walks the tree-sitter concrete syntax tree and produces
typed Rust AST nodes defined in `crates/st-syntax/src/ast.rs`.

```
tree-sitter::Tree + &str  -->  lower::lower()  -->  LowerResult {
                                                       source_file: SourceFile,
                                                       errors: Vec<LowerError>,
                                                     }
```

Design choices:

- Every AST node carries a `TextRange { start, end }` (byte offsets). This
  enables precise diagnostic locations and source-map generation later.
- CST `ERROR` nodes are collected into `LowerError`s but do not halt
  construction. Valid subtrees still produce AST nodes.
- The top-level `SourceFile` contains a `Vec<TopLevelItem>` with variants:
  `Program`, `Function`, `FunctionBlock`, `TypeDeclaration`,
  `GlobalVarDeclaration`.
- The convenience function `st_syntax::parse(source)` creates a parser,
  parses, and lowers in one call.

## Stage 2.5: Multi-File Compilation

Before semantic analysis, the compiler supports **multi-file compilation**
via `parse_multi()`. The standard library is embedded at compile time through
`builtin_stdlib()`, which concatenates all `stdlib/*.st` files into a single
source string. The parser merges the stdlib AST with the user's AST, and only
reports errors from the user source (stdlib parse errors are suppressed).

This means all standard library functions (counters, timers, edge detection,
math) are available in every program without import statements.

## Stage 3: Semantic Analysis

**Crate:** `st-semantics` (`crates/st-semantics/src/analyze.rs`)

Semantic analysis runs in **two passes** over the AST:

### Pass 1 -- Register Top-Level Names

```rust
// analyze.rs
for item in &sf.items {
    self.register_top_level(item);
}
```

This pass inserts every PROGRAM, FUNCTION, FUNCTION_BLOCK, TYPE declaration,
and global VAR block into the global scope of the `SymbolTable`. Forward
references between POUs are therefore resolved correctly.

### Pass 2 -- Analyze Bodies

```rust
for item in &sf.items {
    self.analyze_top_level(item);
}
```

For each POU, the analyzer:

1. Creates a child scope (global -> POU).
2. Registers local variables from VAR / VAR_INPUT / VAR_OUTPUT blocks.
3. Walks the statement list, resolving every variable reference through the
   **scope chain** (`SymbolTable::resolve()` walks from current scope up to
   global).
4. Type-checks expressions using the rules in `types.rs`.

### Intrinsic Function Recognition

The semantic analyzer recognizes **compiler intrinsic functions** by name.
These include:

- **Type conversions** (30+): `INT_TO_REAL`, `REAL_TO_INT`, `BOOL_TO_INT`, etc.
  The compiler recognizes `*_TO_*` patterns and emits `ToInt`, `ToReal`, or
  `ToBool` instructions directly.
- **Trig/math functions** (10): `SQRT`, `SIN`, `COS`, `TAN`, `ASIN`, `ACOS`,
  `ATAN`, `LN`, `LOG`, `EXP`. Each compiles to a dedicated VM instruction.
- **SYSTEM_TIME()**: Compiles to the `SystemTime` VM instruction, returning
  elapsed milliseconds since engine start as a TIME value.

These intrinsics bypass normal function call resolution and are emitted as
single VM instructions for maximum efficiency.

### Scope Chain

```
SymbolTable
  scopes[0]  "global"    -- types, POUs, global vars
  scopes[1]  "Main"      -- locals of PROGRAM Main (parent = 0)
  scopes[2]  "Counter"   -- locals of FB Counter   (parent = 0)
  ...
```

`resolve(scope_id, name)` walks `parent` links until it finds a match or
reaches the root. Names are case-insensitive (uppercased for lookup).

### Type Checking

The semantic type system (`crates/st-semantics/src/types.rs`) defines `Ty`:

| Variant | Description |
|---|---|
| `Elementary(e)` | BOOL, SINT..ULINT, REAL, LREAL, BYTE..LWORD, TIME..LDT |
| `Array { ranges, element_type }` | Multi-dimensional arrays |
| `String { wide, max_len }` | STRING / WSTRING |
| `Struct { name, fields }` | Named struct with `Vec<FieldDef>` |
| `Enum { name, variants }` | Enumeration |
| `Subrange { name, base, lower, upper }` | Constrained integer range |
| `FunctionBlock { name }` | FB instance type |
| `Alias { name, target }` | Type alias (resolved transparently) |
| `Void` / `Unknown` | Programs (no return) / unresolved |

**Widening rules** (`can_coerce`): implicit coercion is allowed when the
source type has a lower `numeric_rank` than the target. The ranking is:
SINT(1) < USINT(2) < INT(3) < UINT(4) < DINT(5) < UDINT(6) < LINT(7) <
ULINT(8) < REAL(9) < LREAL(10). Enum-to-integer coercion is also permitted.

**Common type** (`common_type`): for binary operations, the operand with the
higher rank is selected. If no common type exists the analyzer emits an error
diagnostic.

After both passes, `check_unused()` scans the symbol table for variables that
were declared but never read, emitting warnings.

## Stage 4: IR Compilation

**Crate:** `st-compiler` (`crates/st-compiler/src/compile.rs`)

The compiler translates the AST into register-based bytecode stored in an
`st_ir::Module`.

### Two-Pass Compilation

1. **Register POUs** -- Create empty `Function` entries so that cross-function
   `Call` instructions can resolve indices.
2. **Compile bodies** -- A per-function `FunctionCompiler` allocates registers,
   labels, and local variable slots, then emits instructions.

### Register Allocation

Registers are allocated linearly (`alloc_reg()` increments a counter). Each
expression evaluation returns the `Reg` holding its result. There is no
register reuse or optimization pass; the register file is sized to
`next_reg` at the end of compilation.

### Instruction Set Summary

| Category | Instructions |
|---|---|
| **Register ops** | `Nop`, `LoadConst(dst, val)`, `Move(dst, src)` |
| **Variable access** | `LoadLocal(dst, slot)`, `StoreLocal(slot, src)`, `LoadGlobal(dst, slot)`, `StoreGlobal(slot, src)` |
| **Arithmetic** | `Add`, `Sub`, `Mul`, `Div`, `Mod`, `Pow`, `Neg` |
| **Comparison** | `CmpEq`, `CmpNe`, `CmpLt`, `CmpGt`, `CmpLe`, `CmpGe` |
| **Logic/bitwise** | `And`, `Or`, `Xor`, `Not` |
| **Math intrinsics** | `Sqrt`, `Sin`, `Cos`, `Tan`, `Asin`, `Acos`, `Atan`, `Ln`, `Log`, `Exp` |
| **System** | `SystemTime` |
| **Conversion** | `ToInt`, `ToReal`, `ToBool` |
| **Control flow** | `Jump(label)`, `JumpIf(reg, label)`, `JumpIfNot(reg, label)` |
| **Calls** | `Call { func_index, dst, args }`, `CallFb { instance_slot, func_index, args }`, `Ret(reg)`, `RetVoid` |
| **Aggregate access** | `LoadArray`, `StoreArray`, `LoadField`, `StoreField` |

Total: **48 instruction variants**.

### Source Map Generation

Every `emit()` / `emit_sourced()` call appends a `SourceLocation { byte_offset,
byte_end }` parallel to the instruction vector. The runtime and debugger use
this mapping to translate instruction indices back to source positions.

## Stage 5: Module Output

The final `Module` contains:

```rust
pub struct Module {
    pub functions: Vec<Function>,   // compiled POUs
    pub globals: MemoryLayout,      // global variable slots
    pub type_defs: Vec<TypeDef>,    // struct/enum/array defs for runtime
}
```

Each `Function` carries its name, `PouKind` (Function / FunctionBlock /
Program), register count, instruction vector, label-position map, local
`MemoryLayout`, and source map. The module is serializable via serde for
potential caching or transport.

## Pipeline Summary

```
  .st source
      |
      v
  [tree-sitter]  incremental, error-recovering parse
      |
      v
  CST (tree_sitter::Tree)
      |
      v
  [lower.rs]  walk CST nodes, produce typed AST
      |
      v
  AST (SourceFile)
      |
      v
  [builtin_stdlib() + parse_multi()]  merge stdlib AST with user AST
      |
      v
  [analyze.rs]  pass 1: register names, pass 2: analyze bodies
      |
      v
  SymbolTable + Vec<Diagnostic>
      |
      v
  [compile.rs]  register POUs, then compile to bytecode (intrinsics emitted inline)
      |
      v
  Module { functions, globals, type_defs }
      |
      v
  [vm.rs / engine.rs]  fetch-decode-execute in scan cycles
```
