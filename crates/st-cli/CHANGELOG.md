# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **String manipulation & formatting standard library (Tier 5).** 23 IEC 61131-3 / CODESYS-compatible string intrinsics:
  - Core: `LEN`, `LEFT`, `RIGHT`, `MID`, `CONCAT`, `INSERT`, `DELETE`, `REPLACE` (4-arg), `FIND`
  - Case: `TO_UPPER` / `UPPER_CASE`, `TO_LOWER` / `LOWER_CASE`
  - Trim: `TRIM`, `LTRIM`, `RTRIM`
  - Numeric ↔ STRING: `*_TO_STRING` (signed/unsigned/REAL/BOOL), `STRING_TO_*`, generic `TO_STRING` / `ANY_TO_STRING`
  - Semantics: 1-indexed positions per IEC; out-of-range arguments clamp instead of erroring; `REAL_TO_STRING` preserves the decimal point (`1.0` → `"1.0"`).
- New stdlib doc file `stdlib/strings.st` and a "String Functions" section in the language reference.
- New playground `playground/18_strings.st` covering every Tier 5 function with expected results inline; doubles as the `playground_18_strings_e2e` test (70+ assertions).
- Acceptance tests: 89 unit tests in `string_tests.rs`, 5 LSP unit tests + 3 LSP integration tests (`signatureHelp`, `hover`), 2 DAP integration tests (STRING locals/globals render with IEC single quotes in the variables view and via `evaluate`).

### Changed

- VS Code extension `serverPath` defaults (root + playground `.vscode/settings.json`) now point at `target/container/debug/st-cli` so devcontainer LSP/DAP sessions don't pick up host-built binaries with mismatched glibc. Host workflows still resolve via the extension's auto-search fallback to `target/debug/st-cli`.

### Known limitations

- Each string instruction allocates a fresh `String` (O(n) per op, one heap allocation per call). Fine for typical PLC scan budgets but a candidate for SSO / arena optimisation if a profiled scan shows string ops dominating cycle time.

## [0.3.4](https://github.com/bartei/rust-plc/compare/st-cli-v0.3.3...st-cli-v0.3.4) - 2026-05-02

### Added

- online program/update + full headless VS Code acceptance suite
- *(comm-modbus)* per-device timeout/preamble, error-code split, RTU reliability hardening
- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- wire Modbus RTU into NativeFb registry for ST-level usage
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- auto-build static binary on target install
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- array fields in device profiles with fb.field[i] access
- IEC 61131-3 time/date conversion functions, DATE/TOD/DT literal parsing, date arithmetic
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- array variable support, server-side watch tree, bundled webview
- *(lsp)* method go-to-definition and snippet parameter completion
- *(comm-modbus)* cumulative errors_count VAR on RTU device FB
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- auto-install musl target before building static binary
- auto-build uses cargo directly, falls back to nix-shell
- require plc-project.yaml for bundle command
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle
- member hover for native FB fields, DAP native FB variable expansion
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.3.3
- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1
- phase-1 coverage improvements (65% -> 72%)
- update all references after st-runtime/st-plc-runtime rename
- *(lsp)* cache project files, no disk I/O on keystroke

## [0.3.3](https://github.com/bartei/rust-plc/compare/st-cli-v0.3.2...st-cli-v0.3.3) - 2026-04-27

### Added

- *(comm-modbus)* per-device timeout/preamble, error-code split, RTU reliability hardening
- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- wire Modbus RTU into NativeFb registry for ST-level usage
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- auto-build static binary on target install
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- array fields in device profiles with fb.field[i] access
- IEC 61131-3 time/date conversion functions, DATE/TOD/DT literal parsing, date arithmetic
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- array variable support, server-side watch tree, bundled webview
- *(lsp)* method go-to-definition and snippet parameter completion
- *(comm-modbus)* cumulative errors_count VAR on RTU device FB
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- auto-install musl target before building static binary
- auto-build uses cargo directly, falls back to nix-shell
- require plc-project.yaml for bundle command
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle
- member hover for native FB fields, DAP native FB variable expansion
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1
- phase-1 coverage improvements (65% -> 72%)
- update all references after st-runtime/st-plc-runtime rename
- *(lsp)* cache project files, no disk I/O on keystroke

## [0.3.2](https://github.com/bartei/rust-plc/compare/st-cli-v0.3.1...st-cli-v0.3.2) - 2026-04-24

### Added

- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- wire Modbus RTU into NativeFb registry for ST-level usage
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- auto-build static binary on target install
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- IEC 61131-3 time/date conversion functions, DATE/TOD/DT literal parsing, date arithmetic
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- array variable support, server-side watch tree, bundled webview
- *(lsp)* method go-to-definition and snippet parameter completion
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- auto-install musl target before building static binary
- auto-build uses cargo directly, falls back to nix-shell
- require plc-project.yaml for bundle command
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle
- member hover for native FB fields, DAP native FB variable expansion
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1
- update all references after st-runtime/st-plc-runtime rename
- *(lsp)* cache project files, no disk I/O on keystroke

## [0.3.1](https://github.com/bartei/rust-plc/compare/st-cli-v0.3.0...st-cli-v0.3.1) - 2026-04-23

### Added

- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- wire Modbus RTU into NativeFb registry for ST-level usage
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- auto-build static binary on target install
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- IEC 61131-3 time/date conversion functions, DATE/TOD/DT literal parsing, date arithmetic
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- array variable support, server-side watch tree, bundled webview
- *(lsp)* method go-to-definition and snippet parameter completion
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- auto-install musl target before building static binary
- auto-build uses cargo directly, falls back to nix-shell
- require plc-project.yaml for bundle command
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle
- member hover for native FB fields, DAP native FB variable expansion
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1
- update all references after st-runtime/st-plc-runtime rename
- *(lsp)* cache project files, no disk I/O on keystroke

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-cli-v0.1.2...st-cli-v0.1.3) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- wire Modbus RTU into NativeFb registry for ST-level usage
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- auto-build static binary on target install
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- *(lsp)* method go-to-definition and snippet parameter completion
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- auto-install musl target before building static binary
- auto-build uses cargo directly, falls back to nix-shell
- require plc-project.yaml for bundle command
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle
- member hover for native FB fields, DAP native FB variable expansion
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1
- update all references after st-runtime/st-plc-runtime rename
- *(lsp)* cache project files, no disk I/O on keystroke

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-cli-v0.1.1...st-cli-v0.1.2) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- wire Modbus RTU into NativeFb registry for ST-level usage
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- auto-build static binary on target install
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- *(lsp)* method go-to-definition and snippet parameter completion
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- auto-install musl target before building static binary
- auto-build uses cargo directly, falls back to nix-shell
- require plc-project.yaml for bundle command
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle
- member hover for native FB fields, DAP native FB variable expansion
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- add comprehensive README.md for all 16 crates
- release v0.1.1
- update all references after st-runtime/st-plc-runtime rename
- *(lsp)* cache project files, no disk I/O on keystroke

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-cli-v0.1.0...st-cli-v0.1.1) - 2026-04-06

### Added

- complete Phase 11 — compile-to-file, fmt, and JSON error output
- multi-file workspace support with autodiscovery and plc-project.yaml
- modular standard library with counters, timers, edge detection, math
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
- implement REF_TO pointers with ^ dereference and NULL
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- PLC force/unforce variables and debug toolbar
- Phase 9 — Online Change Manager with hot-reload support
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction

### Fixed

- *(ci)* resolve all clippy warnings for -Dwarnings
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs

### Other

- add 7 local variable retention tests for scan cycle behavior
