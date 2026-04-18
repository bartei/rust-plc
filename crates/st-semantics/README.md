# st-semantics

Semantic analysis and type checking for IEC 61131-3 Structured Text.

## Purpose

Performs semantic analysis on typed ASTs: builds symbol tables, resolves names across scopes, type-checks expressions and assignments, and produces diagnostics. This is the "red squiggly lines" engine for the IDE and the gate-keeper for the compiler.

## Public API

```rust
use st_semantics::{analyze, check};

// Full analysis from AST
let result = analyze::analyze(&source_file);
for diag in &result.diagnostics {
    println!("{}: {}", diag.code, diag.message);
}

// Convenience: parse + analyze in one call
let result = check("PROGRAM Main\nVAR x : INT; END_VAR\nx := TRUE;\nEND_PROGRAM");
// → TypeMismatch diagnostic: cannot assign BOOL to INT
```

### Key Types

- `analyze::AnalysisResult` — Contains `source_file`, `diagnostics`, and `scopes`
- `diagnostic::Diagnostic` — Code, message, severity, and source range
- `diagnostic::DiagnosticCode` — Enum of all diagnostic types (30+)
- `scope::Scope` — Symbol table for a lexical scope

### Diagnostic Categories

- **Name resolution**: `UndeclaredVariable`, `DuplicateDeclaration`, `UndeclaredFunction`
- **Type checking**: `TypeMismatch`, `InvalidOperation`, `IncompatibleTypes`
- **Scope rules**: `VariableNotAccessible`, `InvalidAssignmentTarget`
- **IEC compliance**: `InvalidRetainQualifier`, `MissingReturnValue`

## Functional Description

The analyzer performs a single pass over the AST:

1. **Scope building** — Creates nested scopes for each POU, block, and control structure
2. **Symbol registration** — Registers variables, functions, types in their declaring scope
3. **Name resolution** — Resolves every identifier reference to its declaration
4. **Type checking** — Validates assignments, expressions, function call arguments, and return types
5. **Diagnostic collection** — Accumulates all errors and warnings with precise source locations

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-syntax` | AST types |
| `thiserror` | Error type derivation |

## Tests

Tests cover: type mismatches, undeclared variables, duplicate declarations, scope resolution, function/FB call validation, array indexing, struct field access, pointer dereferencing, and IEC 61131-3 compliance rules.
