# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/bartei/rust-plc/compare/st-engine-v0.3.0...st-engine-v0.3.1) - 2026-04-23

### Added

- IEC 61131-3 time/date conversion functions, DATE/TOD/DT literal parsing, date arithmetic
- array variable support, server-side watch tree, bundled webview
- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle

### Other

- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- update all references after st-runtime/st-plc-runtime rename

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-engine-v0.1.2...st-engine-v0.1.3) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates
- update all references after st-runtime/st-plc-runtime rename

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-engine-v0.1.1...st-engine-v0.1.2) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- multi-rate I/O scheduling — per-device cycle_time enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- forced values on native FB fields survive execute() calls
- critical state bugs in debug attach/detach lifecycle

### Other

- add comprehensive README.md for all 16 crates
- update all references after st-runtime/st-plc-runtime rename

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-engine-v0.1.0...st-engine-v0.1.1) - 2026-04-06

### Added

- implement REF_TO pointers with ^ dereference and NULL
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- modular standard library with counters, timers, edge detection, math
- PLC force/unforce variables and debug toolbar
- Phase 9 — Online Change Manager with hot-reload support
- Phase 8 — DAP debugger with breakpoints, stepping, and variable inspection
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- *(ci)* resolve all clippy warnings for -Dwarnings
- continue runs across multiple scan cycles until breakpoint hit
- debugger breakpoint off-by-one and re-trigger bugs
- retain PROGRAM locals across scan cycles in debugger
- debugger breakpoints, stepping, and source map coverage

### Other

- add 7 local variable retention tests for scan cycle behavior
