# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-dap-v0.1.2...st-dap-v0.1.3) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- multi-rate I/O scheduling — per-device cycle_time enforcement
- unified WebSocket-based PLC Monitor panel
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- PLC force/unforce variables and debug toolbar
- add DAP server diagnostic logging
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- auto-qualify variable names in DAP force/unforce commands
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* comprehensive DAP review and multi-file integration tests
- cross-file go-to-definition and breakpoints in non-main files
- *(dap)* breakpoints now work in multi-file projects
- *(dap)* proper multi-file source mapping for stack traces and breakpoints
- *(dap)* breakpoints now work in multi-file projects
- *(dap)* store main file source for line mapping in multi-file debug
- *(dap)* pass project root directory to discover_project, not .st file
- add multi-file project support to DAP debugger and LSP
- *(ci)* resolve all clippy warnings for -Dwarnings
- stepping at end of cycle wraps to next cycle instead of terminating
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs
- debugger breakpoints, stepping, and source map coverage

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-dap-v0.1.1...st-dap-v0.1.2) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- multi-rate I/O scheduling — per-device cycle_time enforcement
- unified WebSocket-based PLC Monitor panel
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- PLC force/unforce variables and debug toolbar
- add DAP server diagnostic logging
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- auto-qualify variable names in DAP force/unforce commands
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* comprehensive DAP review and multi-file integration tests
- cross-file go-to-definition and breakpoints in non-main files
- *(dap)* breakpoints now work in multi-file projects
- *(dap)* proper multi-file source mapping for stack traces and breakpoints
- *(dap)* breakpoints now work in multi-file projects
- *(dap)* store main file source for line mapping in multi-file debug
- *(dap)* pass project root directory to discover_project, not .st file
- add multi-file project support to DAP debugger and LSP
- *(ci)* resolve all clippy warnings for -Dwarnings
- stepping at end of cycle wraps to next cycle instead of terminating
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs
- debugger breakpoints, stepping, and source map coverage

### Other

- add comprehensive README.md for all 16 crates
- release v0.1.1

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-dap-v0.1.0...st-dap-v0.1.1) - 2026-04-06

### Added

- PLC force/unforce variables and debug toolbar
- add DAP server diagnostic logging
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- *(ci)* resolve all clippy warnings for -Dwarnings
- stepping at end of cycle wraps to next cycle instead of terminating
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs
- debugger breakpoints, stepping, and source map coverage
