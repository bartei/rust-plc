# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-cli-v0.1.1...st-cli-v0.1.2) - 2026-04-13

### Added

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
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* breakpoints now work in multi-file projects
- retain PROGRAM locals across scan cycles in debugger
- *(ci)* remove dead code and unused imports that fail -Dwarnings
- debugger breakpoints, stepping, and source map coverage
- critical state bugs in debug attach/detach lifecycle
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

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
