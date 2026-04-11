# Testing

This chapter covers the testing strategy, how tests are organized, how to run
them, and how to add new tests.

## Overview

The workspace contains **483 tests** across all crates. Every crate with
non-trivial logic has its own test suite. Tests range from unit tests
(individual functions) to integration tests (full parse-analyze-compile-run
round trips).

```bash
# Run the entire test suite
cargo test --workspace
```

## Test Distribution by Crate

| Crate | Test file(s) | Count | What is tested |
|---|---|---|---|
| **st-grammar** | `src/lib.rs` (inline) | 11 | Parser loads, minimal programs, FBs, functions, types, control flow, expressions, literals, comments, error recovery, incremental parse |
| **st-syntax** | `tests/lower_tests.rs` | 21 | CST-to-AST lowering for all node types |
| **st-syntax** | `tests/coverage_gaps.rs` | 58 | Additional lowering edge cases |
| **st-semantics** | `tests/end_to_end_tests.rs` | 17 | Full parse-and-analyze round trips |
| **st-semantics** | `tests/scope_tests.rs` | 22 | Scope creation, resolution, shadowing |
| **st-semantics** | `tests/type_tests.rs` | 38 | Type coercion, common_type, numeric ranking |
| **st-semantics** | `tests/control_flow_tests.rs` | 16 | IF/FOR/WHILE/REPEAT/CASE semantics |
| **st-semantics** | `tests/call_tests.rs` | 13 | Function/FB call argument checking |
| **st-semantics** | `tests/struct_array_tests.rs` | 11 | Struct field access, array indexing, UDTs |
| **st-semantics** | `tests/warning_tests.rs` | 10 | Unused variables, write-without-read |
| **st-semantics** | `tests/coverage_gaps.rs` | 44 | Edge cases for additional coverage |
| **st-lsp** | `tests/lsp_integration.rs` | 13 | Subprocess LSP lifecycle (init, open, diagnostics, shutdown) |
| **st-lsp** | `tests/unit_tests.rs` | 41 | In-process tests for completion, semantic tokens, document sync |
| **st-compiler** | `tests/compile_tests.rs` | 35 | AST-to-IR compilation for all statement/expression types |
| **st-engine** | `tests/vm_tests.rs` | 42 | VM execution: arithmetic, control flow, calls, limits, cycles, intrinsics |
| **st-engine** | `tests/stdlib_tests.rs` | 16 | Standard library integration: counters, timers, edge detection, math |
| **st-engine** | `tests/online_change_tests.rs` | 10 | Engine-level online change: apply, preserve state, reject incompatible |
| **st-engine** | `src/online_change.rs` (inline) | 11 | analyze_change compatibility, migrate_locals state preservation |
| **st-engine** | `src/debug.rs` (inline) | 9 | Debug-mode VM helpers |
| **st-dap** | `tests/dap_integration.rs` | 26 | DAP protocol: breakpoints, stepping, continue across cycles, variables, evaluate, force/unforce |
| **st-monitor** | `tests/monitor_tests.rs` | 4 | WebSocket protocol: connect, subscribe, variable streaming, force/unforce |

## Test Patterns

### Grammar Tests (st-grammar)

Grammar tests are inline in `crates/st-grammar/src/lib.rs`. They verify that
tree-sitter can parse various ST constructs and produce the expected node
structure:

```rust
#[test]
fn test_parse_minimal_program() {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    assert_eq!(program.kind(), kind::PROGRAM_DECLARATION);
}
```

The `test_error_recovery` test confirms that broken syntax still produces a
tree, and `test_incremental_parse` validates that re-parsing after an edit
uses the old tree for efficiency.

### Semantic Tests (st-semantics)

Semantic tests follow a consistent pattern using a `test_helpers` module:

1. Write a complete ST source string.
2. Call the parse-and-analyze pipeline.
3. Assert the expected diagnostics (errors/warnings) by code and/or message.
4. Or assert that zero diagnostics are produced (valid program).

```rust
#[test]
fn test_undeclared_variable() {
    let source = r#"
PROGRAM Main
VAR END_VAR
    x := 1;   // x is not declared
END_PROGRAM
"#;
    let result = check(source);
    assert!(result.diagnostics.iter().any(|d|
        d.message.contains("undeclared")
    ));
}
```

The test files are split by domain: scope resolution, type checking, control
flow validation, function calls, struct/array access, and warnings.

### LSP Tests (st-lsp)

LSP tests come in two flavors:

- **Subprocess tests** (`lsp_integration.rs`, 13 tests): Launch `st-cli serve`
  as a child process, send JSON-RPC messages over stdio, and verify responses.
  These test the full end-to-end LSP protocol including initialization,
  `textDocument/didOpen`, `textDocument/publishDiagnostics`, and shutdown.

- **In-process tests** (`unit_tests.rs`, 41 tests): Directly instantiate the
  `Backend` and call its methods, testing completion results, semantic token
  encoding, and document management without process overhead.

### Compiler Tests (st-compiler)

Compiler tests in `tests/compile_tests.rs` parse ST source, compile it to a
`Module`, and verify the resulting IR structure:

- Correct number of functions in the module.
- Expected instruction sequences for arithmetic, control flow, and calls.
- Proper local/global variable slot allocation.
- Source map entries present for sourced instructions.

### Runtime/VM Tests (st-engine)

VM tests in `tests/vm_tests.rs` compile and execute ST programs, then inspect
the VM state:

```rust
#[test]
fn test_for_loop() {
    let module = compile_source("PROGRAM Main VAR x:INT; i:INT; END_VAR ...");
    let mut vm = Vm::new(module, VmConfig::default());
    vm.run("Main").unwrap();
    assert_eq!(vm.get_global("x"), Some(&Value::Int(55)));
}
```

These tests cover arithmetic operations, comparison, logic, control flow
(IF/FOR/WHILE/REPEAT/CASE), function calls with return values, FB instance
calls, safety limits (stack overflow, execution limit), division by zero,
scan cycle execution through the Engine, and intrinsic functions (trig, math,
conversions, SYSTEM_TIME).

### Standard Library Tests (st-engine)

The `tests/stdlib_tests.rs` file tests the standard library function blocks
end-to-end: counters (CTU, CTD, CTUD) counting on rising edges, timers
(TON, TOF, TP) with TIME values and SYSTEM_TIME(), edge detection (R_TRIG,
F_TRIG), and math functions (MAX_INT, MIN_INT, ABS_INT, LIMIT_INT, etc.).

### DAP Tests (st-dap)

DAP integration tests in `tests/dap_integration.rs` test the full debug
protocol including breakpoints, stepping, continue across scan cycles,
variable inspection, and PLC-specific extensions: `force x = 42`,
`unforce x`, `listForced`, and `scanCycleInfo`.

## Running Individual Test Suites

```bash
# Grammar tests only
cargo test -p st-grammar

# All semantic tests
cargo test -p st-semantics

# Only scope-related semantic tests
cargo test -p st-semantics --test scope_tests

# Only LSP integration tests (these are slower due to subprocess)
cargo test -p st-lsp --test lsp_integration

# Compiler tests
cargo test -p st-compiler

# VM tests
cargo test -p st-engine

# Standard library tests
cargo test -p st-engine --test stdlib_tests

# DAP debugger integration tests
cargo test -p st-dap

# Monitor server tests
cargo test -p st-monitor

# Online change tests (engine-level)
cargo test -p st-engine --test online_change_tests
```

## Code Coverage

The project uses `cargo-llvm-cov` for coverage reporting:

```bash
# Install (one-time)
cargo install cargo-llvm-cov

# Generate an HTML coverage report
cargo llvm-cov --workspace --html

# Generate a summary to the terminal
cargo llvm-cov --workspace

# Open the HTML report
open target/llvm-cov/html/index.html
```

### Current Coverage

Overall workspace coverage is approximately **87%**, with core logic crates
achieving higher:

| Crate | Approximate Coverage |
|---|---|
| st-grammar | ~95% |
| st-syntax (lower.rs) | ~92% |
| st-semantics (analyze.rs) | ~95% |
| st-semantics (types.rs) | ~98% |
| st-semantics (scope.rs) | ~96% |
| st-ir | ~90% |
| st-compiler | ~88% |
| st-engine (vm.rs) | ~91% |
| st-engine (engine.rs) | ~85% |
| st-lsp | ~78% |
| st-cli | ~65% |
| st-dap | ~82% |
| st-monitor | ~80% |

The `coverage_gaps.rs` files in `st-syntax` and `st-semantics` were added
specifically to close coverage holes on edge cases and error paths.

## Adding New Tests

### Adding a Semantic Test

1. Identify the appropriate test file (or create one in
   `crates/st-semantics/tests/`).
2. Write a complete ST source snippet.
3. Call `st_semantics::check(&source)` or the test helper functions.
4. Assert on the diagnostics.

```rust
#[test]
fn test_my_new_check() {
    let source = r#"
PROGRAM Main
VAR
    x : INT;
END_VAR
    x := 3.14;  // should warn about implicit narrowing
END_PROGRAM
"#;
    let result = st_semantics::check(source);
    assert!(result.diagnostics.iter().any(|d|
        d.message.contains("type mismatch")
    ));
}
```

### Adding a Compiler/VM Test

1. Write the ST source.
2. Parse with `st_syntax::parse()`.
3. Compile with `st_compiler::compile()`.
4. Execute with `Vm::new()` + `vm.run()`.
5. Inspect globals or the return value.

### Adding a Grammar Test

Add an inline `#[test]` in `crates/st-grammar/src/lib.rs` that parses a new
construct and asserts the CST structure.

## Continuous Integration

All tests run on every push. The CI pipeline:

1. `cargo fmt --check` -- formatting.
2. `cargo clippy --workspace` -- lints.
3. `cargo test --workspace` -- all 483 tests.
4. `cargo llvm-cov --workspace` -- coverage report (optional).
