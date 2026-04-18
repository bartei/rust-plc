# st-compiler

AST-to-bytecode compiler for IEC 61131-3 Structured Text.

## Purpose

Compiles typed AST nodes into the register-based bytecode instruction set defined by `st-ir`. Handles expression evaluation, control flow lowering, function block instantiation, type coercion, and source map generation for debugging.

## Public API

```rust
use st_compiler::compile;
use st_syntax::parse;

let ast = parse("PROGRAM Main\nVAR x : INT := 0;\nEND_VAR\nx := x + 1;\nEND_PROGRAM");
let module = compile(&ast.source_file).expect("compilation should succeed");

// module.functions contains the compiled "Main" program
// module.globals contains the "x" variable slot
```

- `compile(ast: &SourceFile) -> Result<Module, CompileError>` — Compiles an AST into a bytecode `Module`
- `CompileError` — Error enum: `InvalidType`, `UndeclaredIdentifier`, `InvalidOperation`, etc.

## Functional Description

The compiler performs a single pass over the AST, emitting instructions into per-function instruction buffers:

1. **Global layout** — Allocates global variable slots with offsets and sizes
2. **Function compilation** — For each POU (PROGRAM, FUNCTION, FUNCTION_BLOCK, METHOD):
   - Allocates local variable registers
   - Compiles statements to instruction sequences
   - Emits source map entries for each statement/expression
3. **Expression lowering** — Translates expressions to register operations with implicit type coercion
4. **Control flow** — Lowers `IF/CASE/FOR/WHILE/REPEAT` to `Jump`/`JumpIf` instructions with forward/backward labels
5. **Function calls** — Emits `Call`/`CallFb`/`CallMethod` with register-based argument passing
6. **Global init** — Generates a synthetic `__global_init` function for `VAR_GLOBAL x := <expr>` initializers

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-syntax` | AST types |
| `st-semantics` | Type information for coercion |
| `st-ir` | Target IR types |
| `thiserror` | Error handling |
