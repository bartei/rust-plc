# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
