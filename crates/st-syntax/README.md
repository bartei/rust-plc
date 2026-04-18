# st-syntax

AST types and CST-to-AST lowering for IEC 61131-3 Structured Text.

## Purpose

Converts tree-sitter concrete syntax trees (CSTs) into typed Rust AST nodes with source span tracking. Supports multi-file project loading via `plc-project.yaml` discovery.

This crate is the bridge between the raw parser output and the semantic analysis / compilation pipeline. Every downstream crate works with the typed AST, not with raw CST nodes.

## Public API

```rust
use st_syntax::{parse, ast, multi_file};

// Single-file parse
let result = parse("PROGRAM Main\nVAR x : INT; END_VAR\nEND_PROGRAM");
let source_file = result.source_file;

// Multi-file parse (includes built-in stdlib)
let stdlib = multi_file::builtin_stdlib();
let mut sources: Vec<&str> = stdlib;
sources.push(my_source);
let result = multi_file::parse_multi(&sources);
```

### Key Types

- `ast::SourceFile` — Top-level compilation unit containing `Vec<TopLevelItem>`
- `ast::ProgramDecl`, `FunctionDecl`, `FunctionBlockDecl` — Program Organization Units
- `ast::ClassDecl`, `InterfaceDecl`, `MethodDecl` — OOP constructs
- `ast::Statement`, `ast::Expression` — Statement and expression AST nodes
- `ast::DataType` — All IEC 61131-3 data types
- `ast::TextRange` — Source location (byte offsets) for diagnostics
- `lower::LowerResult` — Parse result containing `source_file` and `errors`

### Multi-File Support

- `multi_file::parse_multi(&[&str])` — Parse multiple source files as a single compilation unit
- `multi_file::builtin_stdlib()` — Returns the built-in standard library source (TON, TOF, TP, CTU, CTD, SR, RS, etc.)
- `project::Project` — Loads project configuration from `plc-project.yaml`

## Configuration

Projects use `plc-project.yaml` for source file discovery:

```yaml
name: MyProject
entryPoint: Main
sources:
  - "*.st"
  - "lib/*.st"
exclude:
  - "test_*.st"
```

When no project file is present, the crate discovers all `.st` files in the current directory.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-grammar` | Tree-sitter parser |
| `tree-sitter` | CST types |
| `serde`, `serde_yaml` | Project YAML parsing |
| `glob` | Source file discovery |

## Tests

Tests cover: single-file parsing, multi-file parsing, project loading, AST node construction, error recovery, and source span tracking.
