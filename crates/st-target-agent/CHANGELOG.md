# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.5](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.3.4...st-target-agent-v0.3.5) - 2026-05-06

### Added

- RETAIN/PERSISTENT UI badge + Phase 2 acceptance coverage
- online program/update + full headless VS Code acceptance suite
- array variable support, server-side watch tree, bundled webview
- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- release
- release v0.3.3
- phase-1 coverage improvements (65% -> 72%)
- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates

## [0.3.4](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.3.3...st-target-agent-v0.3.4) - 2026-05-02

### Added

- online program/update + full headless VS Code acceptance suite
- array variable support, server-side watch tree, bundled webview
- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- release v0.3.3
- phase-1 coverage improvements (65% -> 72%)
- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates

## [0.3.3](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.3.2...st-target-agent-v0.3.3) - 2026-04-27

### Added

- array variable support, server-side watch tree, bundled webview
- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- phase-1 coverage improvements (65% -> 72%)
- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates

## [0.3.2](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.3.1...st-target-agent-v0.3.2) - 2026-04-24

### Added

- array variable support, server-side watch tree, bundled webview
- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates

## [0.3.1](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.3.0...st-target-agent-v0.3.1) - 2026-04-23

### Added

- array variable support, server-side watch tree, bundled webview
- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.1.2...st-target-agent-v0.1.3) - 2026-04-20

### Added

- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-target-agent-v0.1.1...st-target-agent-v0.1.2) - 2026-04-20

### Added

- legacy comm cleanup, hover for FB types, remote debug variable tree
- CI e2e tests for x86_64 and aarch64, nix-based cross-compilation
- native function block communication layer
- add OPC-UA server for HMI/SCADA integration
- DAP proxy single-session enforcement
- fix remote debug attach — source mapping, breakpoints, force, retain
- unified WebSocket-based PLC Monitor panel
- stopOnEntry support + state fixes + E2E tests passing
- Phase 17D — in-process DAP handler for attach-to-running-engine
- Phase 17C — debug command channel for attach-to-running-engine
- Phase 17B — handle VmError::Halt as debug pause, not fatal
- Phase 17A — singleton enforcement via PID file + flock
- Phase 16 — RETAIN/PERSISTENT variable persistence
- add struct variable support + rename st-runtime/st-plc-runtime crates
- journald logging + runtime log level control
- Phase 15 — remote deployment, one-command installer, remote debug, QEMU E2E

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- critical state bugs in debug attach/detach lifecycle
- debug session stability + program reboot persistence + E2E test
- persist program across reboots + debug attach protocol fixes
- three issues — status field mismatch, auto-start, debug session
- non-intrusive debug attach — engine keeps running on connect
- stop running program before debug session + remap breakpoint paths
- resolve clippy warnings across test files

### Other

- add comprehensive README.md for all 16 crates
