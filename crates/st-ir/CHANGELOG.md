# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-ir-v0.1.0...st-ir-v0.1.1) - 2026-04-06

### Added

- implement REF_TO pointers with ^ dereference and NULL
- real-time timers using SYSTEM_TIME() and TIME values
- add trig/math intrinsic functions (SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP)
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- retain PROGRAM locals across scan cycles in debugger
