# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2](https://github.com/bartei/rust-plc/compare/st-syntax-v0.3.1...st-syntax-v0.3.2) - 2026-04-24

### Added

- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- modular standard library with counters, timers, edge detection, math
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings

### Other

- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1

## [0.3.1](https://github.com/bartei/rust-plc/compare/st-syntax-v0.3.0...st-syntax-v0.3.1) - 2026-04-23

### Added

- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- modular standard library with counters, timers, edge detection, math
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings

### Other

- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-syntax-v0.1.2...st-syntax-v0.1.3) - 2026-04-20

### Added

- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- modular standard library with counters, timers, edge detection, math
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates
- release v0.1.1

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-syntax-v0.1.1...st-syntax-v0.1.2) - 2026-04-20

### Added

- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- implement IEC 61131-3 partial variable access (.%X, .%B, .%W, .%D)
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- modular standard library with counters, timers, edge detection, math
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- remove unused import in lower.rs tests (CI -D warnings)
- *(ci)* resolve all clippy warnings for -Dwarnings

### Other

- add comprehensive README.md for all 16 crates
- release v0.1.1

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-syntax-v0.1.0...st-syntax-v0.1.1) - 2026-04-06

### Added

- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- modular standard library with counters, timers, edge detection, math
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- *(ci)* resolve all clippy warnings for -Dwarnings
