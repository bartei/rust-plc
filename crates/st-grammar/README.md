# st-grammar

Tree-sitter parser for the IEC 61131-3 Structured Text language.

## Purpose

Provides a high-performance, incremental parser generated from a tree-sitter grammar definition. This is the foundation of the entire compilation and analysis pipeline — every source file passes through this parser first.

The grammar is case-insensitive (as required by IEC 61131-3) and supports error recovery, meaning partial or malformed source files still produce a usable parse tree for IDE features.

## Public API

```rust
use st_grammar::{language, kind};

// Get the tree-sitter Language object
let lang = language();

// Node kind constants for matching CST nodes
assert_eq!(kind::PROGRAM_DECLARATION, "program_declaration");
assert_eq!(kind::IF_STATEMENT, "if_statement");
```

- `language()` — Returns the tree-sitter `Language` for Structured Text
- `kind` module — String constants for all 60+ CST node types (program declarations, function blocks, statements, expressions, literals, etc.)

## Functional Description

The grammar handles:
- **Program Organization Units**: `PROGRAM`, `FUNCTION`, `FUNCTION_BLOCK`
- **OOP extensions**: `CLASS`, `INTERFACE`, `METHOD`, `PROPERTY`
- **Data types**: All IEC elementary types (`BOOL`, `INT`, `REAL`, `STRING`, `TIME`, etc.), arrays, structs, enums, pointers (`REF_TO`)
- **Statements**: `IF/ELSIF/ELSE`, `CASE`, `FOR`, `WHILE`, `REPEAT`, assignments, function calls
- **Expressions**: Arithmetic, comparison, logical, bitwise, parenthesized, function calls, array/struct access
- **Declarations**: `VAR`, `VAR_INPUT`, `VAR_OUTPUT`, `VAR_IN_OUT`, `VAR_GLOBAL`, `VAR_EXTERNAL`, `RETAIN`, `PERSISTENT`
- **Comments**: `(* block *)` and `// line`
- **Error recovery**: Produces partial trees for incomplete/invalid source

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tree-sitter` | Parser runtime |
| `cc` (build) | Compiles generated C parser source |

## Build

The `build.rs` script compiles the tree-sitter grammar's generated C code into a static library linked into the Rust crate. No external tools are needed at build time beyond a C compiler.

## Tests

10 tests covering: minimal programs, function blocks, type declarations, control flow, expressions, literals, comments, error recovery, and incremental re-parsing.
