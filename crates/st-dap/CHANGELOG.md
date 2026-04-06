# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
