# Communication Layer — Progress Tracker

> **Design document:** [design_comm.md](design_comm.md) — architecture, examples, rationale.
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.

---

## Native Function Block Infrastructure

### NativeFb Trait and Registry (`st-comm-api`)

- [x] `NativeFb` trait: `type_name()`, `layout()`, `execute()`
- [x] `NativeFbLayout`, `NativeFbField`, `NativeFbVarKind` types
- [x] `NativeFbRegistry`: register, find (case-insensitive), all, is_empty
- [x] `DeviceProfile::to_native_fb_layout()` — profile → layout conversion
- [x] `field_data_type_to_var_type()` / `field_data_type_to_int_width()` helpers
- [x] `layout_to_memory_layout()` — NativeFbLayout → st_ir::MemoryLayout
- [x] Unit tests: registry operations, profile-to-layout, type mappings

### Semantic Analyzer (`st-semantics`)

- [x] `analyze_with_native_fbs()` entry point (backward-compatible)
- [x] `register_native_fbs()` — injects native FB types as `SymbolKind::FunctionBlock`
- [x] `field_data_type_to_semantic_ty()` — FieldDataType → Ty mapping
- [x] VarInput fields → params, Var fields → outputs (for dot-access resolution)
- [x] Tests: field access, unknown field error, undeclared without registry

### Compiler (`st-compiler`)

- [x] `compile_with_native_fbs()` entry point (backward-compatible)
- [x] Synthetic `Function` entries with `PouKind::FunctionBlock`, correct `MemoryLayout`
- [x] `native_fb_indices: Vec<u16>` added to `st_ir::Module`
- [x] Native FBs registered before Pass 1 (so `VarType::FbInstance` resolves)
- [x] Tests: instance compilation, field access (LoadField/StoreField)

### VM Dispatch (`st-engine`)

- [x] `Vm::new_with_native_fbs()` constructor
- [x] `call_fb()` checks `native_fb_indices`, dispatches to `execute()`
- [x] Instance state persists in `fb_instances` (same as normal FBs)
- [x] No call frame pushed for native FBs (synchronous Rust execution)
- [x] `Engine::new_with_native_fbs()` constructor
- [x] Tests: execute called, state persists, input params, field write, multiple instances

### Simulated Device (`st-comm-sim`)

- [x] `SimulatedNativeFb` wrapper with cached `NativeFbLayout`
- [x] `NativeFb` impl: bridges `Value` ↔ `IoValue` for web UI state
- [x] `io_value_to_vm_value()` / `vm_value_to_io_value()` conversion functions
- [x] Web UI shared state (`Arc<Mutex<HashMap>>`) works through `execute()`
- [x] Existing `CommDevice` impl preserved (legacy path still works)

---

## Pipeline Wiring

### CLI (`st-cli`)

- [x] `load_native_fbs_for_project()` — discovers profiles, builds registry
- [x] `discover_all_profiles()` — scans profile search paths
- [x] `start_native_web_uis()` — spawns web UIs for native FB devices
- [x] `run` command: passes registry to analyze, compile, and engine
- [x] `check` command: passes registry to analyze
- [x] `bundle` command: pass registry to compile
- [ ] `compile` command: pass registry to compile
- [ ] `fmt` command: pass registry to analyze

### LSP (`st-lsp`)

- [x] `build_native_fb_registry()` — discovers profiles from project root
- [x] `analyze_with_cached_project()` — passes registry to analysis
- [x] `analyze_source_with_uri()` — passes registry to analysis
- [x] Dot-completion works for native FB fields (automatic — same symbol table)
- [x] Hover works for native FB types (automatic)
- [x] Type checking works for native FB field access (automatic)

### DAP (`st-dap`)

- [x] Pass registry to `compile_with_native_fbs()` in `handle_launch()`
- [x] Variable expansion works for native FB instances (automatic — same MemoryLayout)
- [x] FB summary display works (automatic — same `fb_summary_value()` path)

### Runtime (`st-runtime`)

- [x] Pass registry to analyze/compile/engine in target agent
- [x] Persist profiles from bundle to disk (`current_profiles/`)
- [x] Build NativeFbRegistry at program start from persisted profiles
- [x] Bundle includes profiles from parent directories (workspace root pattern)
- [x] E2E verified: execute() runs on QEMU target, connected=TRUE, io_cycles advances
- [x] E2E verified: force DI_0→DO_0 I/O flow through program logic on target

---

## Integration Tests

- [x] `native_fb_test.rs`: 5 tests (execute, state persistence, params, field write, multi-instance)
- [x] `native_fb_integration.rs`: 3 tests (profile roundtrip, multiple devices, diagnostics)
- [x] Semantic tests: 3 tests (field access, unknown field, undeclared type)
- [x] Compiler tests: 2 tests (instance compilation, field access)

---

## Playground Examples

- [x] `playground/sim_project/` — converted to native FB syntax (Sim8DI4AI4DO2AO + SimVfd)
- [x] Merged `native_fb_demo/` into `sim_project/` (removed redundant playground)

---

## Legacy Cleanup (COMPLETED)

- [x] Removed `_io_map.st` codegen (`st-comm-api/src/codegen.rs` deleted)
- [x] Removed `CommManager` (`st-engine/src/comm_manager.rs` deleted)
- [x] Removed `CommConfig`/`LinkConfig`/`DeviceConfig` from config.rs
- [x] Removed `CommDevice`/`CommLink` traits (`device.rs`, `link.rs` deleted)
- [x] Removed `SimulatedLink` (`st-comm-sim/src/link.rs` deleted)
- [x] Removed old `comm_setup` from CLI and DAP (load_for_project, register_simulated_devices)
- [x] Removed `_io_map.st` from bundle system
- [x] Removed `comm-gen` CLI command
- [x] Updated all tests (profile_integration, bundle_e2e, sim device tests)

---

## RS-485 Serial Link

> **Design:** [design_comm.md](design_comm.md) — RS-485 Serial Link section

### New crate: `st-comm-serial`

- [ ] Create `crates/st-comm-serial/` with `Cargo.toml`
- [ ] Add `serialport` crate dependency (cross-platform serial I/O)
- [ ] Implement `SerialTransport` — opens/closes serial port, send/receive bytes
- [ ] RS-485 half-duplex support: Linux `RS485` ioctl for DE/RE pin control
- [ ] Bus access mutex: `Arc<Mutex<SerialTransport>>` shared across devices
- [ ] Inter-frame timing: enforce 3.5-character silent interval between frames

### SerialLink NativeFb

- [ ] Implement `SerialLinkNativeFb` (NativeFb trait)
- [ ] VAR_INPUT: `port`, `baud`, `parity`, `data_bits`, `stop_bits`
- [ ] VAR: `connected`, `error_code`
- [ ] `execute()`: open port on first call, maintain connection, set diagnostics
- [ ] Expose `Arc<Mutex<SerialTransport>>` for device FBs via link handle
- [ ] Reconnect with backoff on port errors

### Tests

- [ ] Unit tests: frame timing, bus mutex, connect/reconnect logic
- [ ] Integration test with virtual serial port pair (`socat` pty)
- [ ] Test on Raspberry Pi: `/dev/ttyAMA0` (built-in UART) and `/dev/ttyUSB0` (USB adapter)

---

## Modbus RTU Protocol

> **Design:** [design_comm.md](design_comm.md) — Modbus RTU Protocol section

### New crate: `st-comm-modbus`

- [ ] Create `crates/st-comm-modbus/` with `Cargo.toml`
- [ ] Modbus RTU frame builder: slave address + function code + data + CRC16
- [ ] Modbus RTU frame parser: validate CRC, extract response data
- [ ] CRC16 calculation (Modbus polynomial)
- [ ] Supported function codes:
  - [ ] FC01 Read Coils
  - [ ] FC02 Read Discrete Inputs
  - [ ] FC03 Read Holding Registers
  - [ ] FC04 Read Input Registers
  - [ ] FC05 Write Single Coil
  - [ ] FC06 Write Single Register
  - [ ] FC15 Write Multiple Coils
  - [ ] FC16 Write Multiple Registers
- [ ] Exception response parsing (error codes 01-06)
- [ ] Register grouping optimizer: merge consecutive registers into multi-read/write

### ModbusRtuDevice NativeFb

- [ ] Implement `ModbusRtuDeviceNativeFb` (NativeFb trait)
- [ ] VAR_INPUT: `link` (SerialLink handle), `slave_id`, `refresh_rate`
- [ ] VAR: `connected`, `error_code`, `io_cycles`, `last_response_ms`, + profile fields
- [ ] `execute()` flow:
  1. [ ] Check refresh_rate timing (skip if not elapsed)
  2. [ ] Read inputs: group registers by kind → build FC01/02/03/04 requests → send via link → parse response → write to field slots
  3. [ ] Write outputs: read field slots → build FC05/06/15/16 requests → send via link
  4. [ ] Apply register scaling/offset from profile
  5. [ ] Update diagnostics (connected, error_code, io_cycles, last_response_ms)
- [ ] Timeout handling: 100ms default, configurable per-device
- [ ] Retry logic: 1 retry on timeout, then mark disconnected

### Device Profiles for Real Hardware

- [ ] `profiles/wago_750_352.yaml` — WAGO 750-352 I/O coupler (16 DI/DO)
- [ ] `profiles/abb_acs580.yaml` — ABB ACS580 VFD (speed ref, feedback, status)
- [ ] `profiles/siemens_g120.yaml` — Siemens G120 VFD
- [ ] `profiles/generic_modbus_16di.yaml` — Generic 16-channel digital input
- [ ] `profiles/generic_modbus_8do.yaml` — Generic 8-channel digital output
- [ ] `profiles/pt100_4ch.yaml` — 4-channel PT100 temperature sensor

### Tests

- [ ] Unit tests: CRC16, frame build/parse, exception handling, register grouping
- [ ] Integration test: mock serial loopback with Modbus slave simulator
- [ ] E2E test: QEMU VM with `socat` virtual serial + Modbus slave emulator
- [ ] Raspberry Pi test: real RS-485 adapter + WAGO I/O module

---

## Future Work

### Modbus TCP

- [ ] `TcpLink` NativeFb (TCP socket management)
- [ ] `ModbusTcpDevice` NativeFb (Modbus TCP/IP, MBAP header instead of CRC)
- [ ] Device profiles same as RTU (register map is protocol-independent)

### Plugin System

- [ ] `plugin.yaml` schema definition
- [ ] `plugins:` section in `plc-project.yaml`
- [ ] `st-cli plugin fetch/update/list` commands
- [ ] `.st-plugins.lock` lockfile for reproducible builds

### Advanced Features

- [ ] Generate stub `.st` files for go-to-definition on native FB types
- [ ] WASM protocol plugins for third-party proprietary protocols
- [ ] Global FB instances (for multi-program device sharing)
- [ ] Online change with native FB state migration
