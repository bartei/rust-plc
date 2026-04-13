# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-deploy-v0.1.1...st-deploy-v0.1.2) - 2026-04-13

### Added

- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve clippy warnings across test files

### Other

- remove password auth + SSH tunnel management
