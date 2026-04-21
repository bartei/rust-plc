# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-runtime-v0.1.2...st-runtime-v0.1.3) - 2026-04-20

### Added

- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- Phase 17A — singleton enforcement via PID file + flock
- add struct variable support + rename st-runtime/st-plc-runtime crates
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- modular standard library with counters, timers, edge detection, math
- PLC force/unforce variables and debug toolbar
- Phase 9 — Online Change Manager with hot-reload support
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- three issues — status field mismatch, auto-start, debug session
- resolve clippy warnings across test files
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* replace unreliable byte-offset filter with name-based matching
- *(dap)* breakpoints now work in multi-file projects
- *(dap)* breakpoints now work in multi-file projects
- add multi-file project support to DAP debugger and LSP
- *(ci)* resolve all clippy warnings for -Dwarnings
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs
- retain PROGRAM locals across scan cycles in debugger
- debugger breakpoints, stepping, and source map coverage

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1
- add 7 local variable retention tests for scan cycle behavior

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-runtime-v0.1.1...st-runtime-v0.1.2) - 2026-04-20

### Added

- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- Phase 17A — singleton enforcement via PID file + flock
- add struct variable support + rename st-runtime/st-plc-runtime crates
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(phase13a.2/3)* cycle-time control, jitter, live monitor + watch list
- *(phase13a)* simulated device, web UI, and on-disk I/O symbol map
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- modular standard library with counters, timers, edge detection, math
- PLC force/unforce variables and debug toolbar
- Phase 9 — Online Change Manager with hot-reload support
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- three issues — status field mismatch, auto-start, debug session
- resolve clippy warnings across test files
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(vm)* SINT/INT/DINT wrap on overflow + literal context typing + monitor polish
- *(dap)* replace unreliable byte-offset filter with name-based matching
- *(dap)* breakpoints now work in multi-file projects
- *(dap)* breakpoints now work in multi-file projects
- add multi-file project support to DAP debugger and LSP
- *(ci)* resolve all clippy warnings for -Dwarnings
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs
- retain PROGRAM locals across scan cycles in debugger
- debugger breakpoints, stepping, and source map coverage

### Other

- add comprehensive README.md for all 16 crates
- release v0.1.1
- add 7 local variable retention tests for scan cycle behavior
