# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-syntax-v0.1.0...st-syntax-v0.1.1) - 2026-04-06

### Added

- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- add full type conversion intrinsics (*_TO_INT, *_TO_REAL, *_TO_BOOL)
- modular standard library with counters, timers, edge detection, math
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- *(ci)* resolve all clippy warnings for -Dwarnings
