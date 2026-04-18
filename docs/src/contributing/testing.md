# Testing

This chapter covers the testing strategy, how tests are organized, how to run
them, and how to add new tests.

## Overview

The workspace contains **500+ tests** across all crates, plus **25 QEMU e2e
tests** that deploy to real virtual machines. Tests range from unit tests
(individual functions) to integration tests (full parse-analyze-compile-run
round trips) to end-to-end deployment tests on x86_64 and aarch64 targets.

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
| **st-semantics** | `tests/call_tests.rs` | 16 | Function/FB call argument checking, native FB type injection |
| **st-semantics** | `tests/struct_array_tests.rs` | 11 | Struct field access, array indexing, UDTs |
| **st-semantics** | `tests/warning_tests.rs` | 10 | Unused variables, write-without-read |
| **st-semantics** | `tests/coverage_gaps.rs` | 44 | Edge cases for additional coverage |
| **st-lsp** | `tests/lsp_integration.rs` | 13 | Subprocess LSP lifecycle (init, open, diagnostics, shutdown) |
| **st-lsp** | `tests/unit_tests.rs` | 41 | In-process tests for completion, semantic tokens, document sync |
| **st-compiler** | `tests/compile_tests.rs` | 37 | AST-to-IR compilation, native FB synthetic entries |
| **st-engine** | `tests/vm_tests.rs` | 42 | VM execution: arithmetic, control flow, calls, limits, cycles, intrinsics |
| **st-engine** | `tests/stdlib_tests.rs` | 16 | Standard library integration: counters, timers, edge detection, math |
| **st-engine** | `tests/native_fb_test.rs` | 5 | Native FB dispatch: execute, state, params, field write, multi-instance |
| **st-engine** | `tests/native_fb_integration.rs` | 3 | Native FB end-to-end: profile roundtrip, multiple devices, diagnostics |
| **st-engine** | `tests/online_change_tests.rs` | 10 | Engine-level online change: apply, preserve state, reject incompatible |
| **st-engine** | `src/online_change.rs` (inline) | 11 | analyze_change compatibility, migrate_locals state preservation |
| **st-engine** | `src/debug.rs` (inline) | 9 | Debug-mode VM helpers |
| **st-dap** | `tests/dap_integration.rs` | 26 | DAP protocol: breakpoints, stepping, continue across cycles, variables, evaluate, force/unforce |
| **st-monitor** | `tests/monitor_tests.rs` | 4 | WebSocket protocol: connect, subscribe, variable streaming, force/unforce |
| **st-target-agent** | `tests/e2e_qemu.rs` | 25 | QEMU VM deployment: x86_64 (21) + aarch64 (4), gated by `ST_E2E_QEMU=1` |
| **st-comm-api** | `src/native_fb.rs` (inline) | 7 | NativeFb registry, profile-to-layout, type mappings |

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

## End-to-End Tests (QEMU Virtual PLC)

The project includes a full QEMU-based end-to-end test suite that boots real
virtual machines, deploys the PLC agent binary via SSH, and exercises the
complete deployment pipeline. These tests verify that compilation, bundling,
deployment, program execution, variable monitoring, and remote debugging all
work on actual Linux targets.

**Not run during normal `cargo test`** — gated by `ST_E2E_QEMU=1`.

### Prerequisites

| Requirement | x86_64 | aarch64 |
|-------------|--------|---------|
| QEMU | `qemu-system-x86_64` (system package) | `qemu-system-aarch64` (`nix-shell -p qemu`) |
| KVM | `/dev/kvm` required | Not needed (software emulation) |
| Cloud image | `setup-images.sh x86_64` | `setup-images.sh aarch64` + UEFI firmware |
| Static binary | `scripts/build-static.sh` | `scripts/build-static.sh aarch64` |
| Rust target | `x86_64-unknown-linux-musl` | `aarch64-unknown-linux-musl` |

### Setup (one-time)

```bash
# 1. Install Rust musl targets
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl   # only if testing aarch64

# 2. Download VM images and generate SSH keys
cd tests/e2e-deploy/vm
./setup-images.sh x86_64

# For aarch64 (optional):
./setup-images.sh aarch64
# Copy UEFI firmware from nix if not found on system:
nix-shell -p qemu --run "cp \$(find /nix/store -maxdepth 4 \
    -name 'edk2-aarch64-code.fd' | head -1) images/QEMU_EFI.fd"

# 3. Build static musl binaries (agent + CLI)
cd <project-root>
scripts/build-static.sh              # x86_64
scripts/build-static.sh aarch64      # aarch64 (optional)
```

### Running x86_64 tests

```bash
# All x86_64 tests (21 tests, ~10 minutes)
ST_E2E_QEMU=1 cargo test -p st-target-agent --test e2e_qemu -- --test-threads=1

# Single test with output
ST_E2E_QEMU=1 cargo test -p st-target-agent --test e2e_qemu \
    e2e_x86_64_native_fb_deploy_and_run -- --nocapture

# Just the deployment/lifecycle tests (skip DAP debug tests)
ST_E2E_QEMU=1 cargo test -p st-target-agent --test e2e_qemu \
    e2e_x86_64_upload -- --nocapture
```

### Running aarch64 tests

aarch64 tests require `qemu-system-aarch64` (available via nix) and use
software emulation (~10x slower than KVM). Run from a nix-shell:

```bash
# All aarch64 tests (4 tests, ~6 minutes)
nix-shell -p qemu --run \
    "ST_E2E_QEMU=1 ST_E2E_AARCH64=1 cargo test -p st-target-agent \
     --test e2e_qemu e2e_aarch64 -- --nocapture --test-threads=1"
```

### Running all tests (both architectures)

```bash
nix-shell -p qemu --run \
    "ST_E2E_QEMU=1 ST_E2E_AARCH64=1 cargo test -p st-target-agent \
     --test e2e_qemu -- --nocapture --test-threads=1"
```

### Test inventory

**x86_64** (21 tests):
- Bootstrap and health check
- Target info (verifies x86_64 arch)
- Upload bundle, start/stop/restart, delete program
- Health while running, logs endpoint
- Online update with compatible (v2) and incompatible (v3) layouts
- **Native FB deploy and run** — profile-based device FB project
- Remote debug via direct port, SSH tunnel, release rejection, session update

**aarch64** (4 tests):
- Bootstrap and health check
- Upload and run
- Full lifecycle (upload → start → verify → stop → delete)
- **Native FB deploy and run** — cross-compiled ARM64 native FB project

### Test fixtures

| Fixture | Description | Used by |
|---------|-------------|---------|
| `test-project/` | Counter program + FB helper (v1.0.0) | Most x86_64/aarch64 tests |
| `test-project-v2/` | Compatible update (counter += 2) | Online update tests |
| `test-project-v3/` | Incompatible layout change | Incompatible update test |
| `test-native-fb/` | Native FB with `SimpleIO` device profile | Native FB e2e tests |

### How it works

Each test:
1. Boots a fresh QEMU VM from a Debian 12 cloud image (copy-on-write overlay)
2. Waits for SSH to accept authenticated connections
3. Uploads the static agent binary via SCP
4. Writes agent config and starts the agent in background
5. Polls the agent's HTTP health endpoint until ready
6. Creates a `.st-bundle` from the test fixture
7. Uploads the bundle via HTTP, starts the program, verifies execution
8. Cleans up (VM is stopped automatically on test completion via `Drop`)

### Troubleshooting

**"Agent did not become ready"** — OPC-UA certificate generation takes ~6s on
x86_64/KVM and ~25s on aarch64/emulation. The test waits up to 30s (x86_64) or
60s (aarch64). If this times out, check the agent log:
```bash
ssh -i tests/e2e-deploy/vm/images/test_key -p 2222 plc@127.0.0.1 \
    "cat /var/log/st-agent/stdout.log"
```

**"Connection reset by peer" on SCP** — SSH port is open but `sshd` isn't ready.
The `wait-ssh.sh` script verifies actual SSH authentication, not just port
availability. If you see this error, the VM may need more boot time.

**Static binary required** — The VM runs Debian 12 (glibc 2.36). Debug builds
link against the host's glibc (likely newer), causing "GLIBC not found" errors.
Always build static musl binaries for e2e tests.

**aarch64 "file in wrong format"** — Cross-compilation needs
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-unknown-linux-musl-gcc`.
The `scripts/build-static.sh aarch64` script sets this automatically.

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
3. `cargo test --workspace` -- all unit + integration tests.
4. `cargo llvm-cov --workspace` -- coverage report (optional).

QEMU e2e tests are not part of the CI pipeline (require KVM hardware). Run
them manually before releases or on dedicated CI runners with `/dev/kvm`.
