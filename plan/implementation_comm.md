# Communication Layer — Progress Tracker

> **Design document:** [design_comm.md](design_comm.md) — architecture, examples, rationale.
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.

---

## Native Function Block Infrastructure

### NativeFb Trait and Registry (`st-comm-api`)

- [x] `NativeFb` trait: `type_name()`, `layout()`, `execute()`
- [x] `NativeFbLayout`, `NativeFbField`, `NativeFbVarKind` types
- [x] `NativeFbRegistry`: register, find (case-insensitive), all, is_empty
- [x] `DeviceProfile::to_native_fb_layout()` — simulated profile → layout conversion
- [x] `DeviceProfile::to_modbus_rtu_device_layout()` — modbus-rtu profile → layout (link, slave_id, refresh_rate, timeout, preamble, diagnostics, profile fields)
- [x] `field_data_type_to_var_type()` / `field_data_type_to_int_width()` helpers
- [x] `layout_to_memory_layout()` — NativeFbLayout → st_ir::MemoryLayout (with TypeDef generation for array fields)
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
- [x] `native_fb_integration.rs`: 4 tests (profile roundtrip, multiple devices, diagnostics, modbus-rtu layout slots)
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

## RS-485 Serial Link (COMPLETED)

- [x] `st-comm-serial` crate with `serialport` dependency
- [x] `SerialTransport`: open/close, send/receive, inter-frame timing (3.5 char gap)
- [x] RS-485 hardware direction control (automatic for USB adapters)
- [x] Bus access via `Arc<Mutex<SerialTransport>>`
- [x] `SerialLinkNativeFb`: NativeFb with port/baud/parity/data_bits/stop_bits params
- [x] Connection lifecycle: open on first call, reconnect on loss
- [x] 11 unit tests + 5 integration tests with socat virtual serial pairs

---

## Modbus RTU Protocol (COMPLETED)

- [x] `st-comm-modbus` crate
- [x] CRC16 (Modbus polynomial 0xA001)
- [x] Frame builder/parser for FC01-FC06, FC0F, FC10
- [x] Exception response parsing
- [x] `RtuClient`: high-level API (read_coils, read_holding_registers, write_single_coil, etc.)
- [x] `ModbusRtuDeviceNativeFb`: generic device FB parameterized by YAML profile
- [x] Register scaling/offset from profile
- [x] Multi-rate I/O via refresh_rate
- [x] 19 unit tests + 9 integration tests with socat Modbus slave simulator

### Remaining

- [x] Register grouping optimizer (merge consecutive registers into multi-read/write)
- [x] Batched coil writes (consecutive coils via FC0F instead of individual FC05)
- [x] Per-device `timeout` VAR_INPUT — overrides `DEFAULT_TIMEOUT` (100 ms)
- [x] Per-device `preamble` VAR_INPUT — minimum bus-silence before each tx, on top of the 3.5-char gap (default 5 ms; eliminates silent request-drops with cheap RS-485 slaves)
- [x] Diagnostic `error_code` split into `ERR_TIMEOUT` (10), `ERR_CRC` (11), `ERR_SLAVE_MISMATCH` (12), `ERR_FC_MISMATCH` (13), `ERR_MODBUS_EXCEPTION` (14), `ERR_OTHER` (15) — exposed via `st_comm_modbus::device_fb`
- [x] Cumulative `errors_count : UDINT` Var — increments once per poll cycle when `error_code != 0`. `saturating_add` so it freezes at `u64::MAX` instead of wrapping. Useful for long-running deployments where transient issues would otherwise be invisible by the time the operator notices.
- [x] Slave-id + FC validation in `RtuFrameParser::for_request` — rejects stale frames from other slaves (or our own echo) before they corrupt the response
- [x] OS input + output buffer purge (`ClearBuffer::All`) at the start of every `transaction_framed`, with hex-logged discard so flushed bytes are observable
- [x] Per-transaction OS read timeout (`port.set_timeout`) reconciled with the caller's deadline so short transaction timeouts aren't stretched
- [x] Tracing instrumentation: `tx ok`/`tx timeout`/`tx invalid` lines with hex dumps of request + (partial) response, and per-transaction send/recv timing
- [ ] Retry logic (1 retry on transient errors — ERR_TIMEOUT/CRC/MISMATCH classes)

### Two-Layer Architecture (COMPLETED)

- [x] SerialLink auto-registered when modbus-rtu profiles are present
- [x] Two-layer model: SerialLink (transport) + device FB (protocol)
- [x] Device takes `link : STRING` parameter (port path from SerialLink)
- [x] `BusManager` in `st-comm-serial/src/bus.rs` — one I/O thread per serial port
- [x] `BusDeviceIo` trait — protocol-agnostic bus device interface
- [x] Non-blocking scan cycle: execute() copies cached values, never touches serial port
- [x] Batched register reads/writes (consecutive registers in single Modbus transaction)
- [x] Round-robin device polling respecting per-device refresh_rate
- [x] Multiple devices share one bus thread (no half-duplex contention)
- [x] SerialLink uses AtomicBool for connection state (no transport lock on scan cycle)
- [x] Shared transport map for link-device binding
- [x] SerialLink registered in LSP/DAP for completions, hover, type checking
- [x] Full-stack test: SerialLink + device with `link := serial.port` → socat → verify
- [x] Playground: `playground/modbus_demo/` with Waveshare profiles + two-layer ST
- [x] Manual test: real hardware with two slaves on one RS-485 bus (verified)
- [ ] E2E test: QEMU VM with `socat` virtual serial + Modbus slave emulator
- [ ] Raspberry Pi test: real RS-485 adapter + WAGO I/O module

---

## Modbus TCP Protocol (COMPLETED)

- [x] `st-comm-modbus-tcp` crate (self-contained, independent of `st-comm-serial`/`st-comm-modbus`)
- [x] `TcpTransport`: connect/disconnect, send/receive_exact, auto-reconnect on failure
- [x] MBAP frame builder/parser (transaction ID, protocol ID, length, unit ID)
- [x] `TcpModbusClient`: high-level API (read_coils, read_holding_registers, write_single_coil, etc.)
- [x] `ModbusTcpDeviceNativeFb`: generic device FB with dedicated I/O thread per device
- [x] Unified transport+protocol: device FB owns its own TCP connection (no separate TcpLink)
- [x] Batched register reads/writes (consecutive registers in single Modbus transaction)
- [x] Batched coil writes (consecutive coils via FC0F instead of individual FC05)
- [x] `DeviceProfile::to_modbus_tcp_device_layout()` in `st-comm-api`
- [x] Registration in CLI (`comm_setup.rs`), LSP (`document.rs`), DAP (`server.rs`)
- [x] Device profiles use `protocol: modbus-tcp` (register map is protocol-independent)
- [x] 25 unit tests: frame building/parsing, MBAP header, transport, layout, slot constants
- [x] Playground: `playground/modbus_tcp_demo/` with Waveshare 8-relay profile
- [x] Manual test: real hardware with Waveshare 8-relay output over TCP (verified)

---

## Batched Coil Writes (COMPLETED)

- [x] Modbus RTU: consecutive output coils batched via FC0F (write_multiple_coils)
- [x] Modbus TCP: consecutive output coils batched via FC0F (write_multiple_coils)
- [x] Single coils still use FC05 (write_single_coil) for efficiency

---

## Array Fields in Device Profiles (COMPLETED)

- [x] `ProfileField.count` — declares N consecutive registers as one array field
- [x] `NativeFbField.dimensions` — array type info for layout/compiler/semantics
- [x] `layout_to_memory_layout()` — creates `TypeDef::Array`, inline expansion (Value-count offsets)
- [x] `LoadFieldIndex` / `StoreFieldIndex` IR instructions for `fb.field[i]` access
- [x] Semantic analyzer: registers array fields as `Ty::Array` (3-part chains already work)
- [x] Compiler: 3-part access chains (`fb.field[i]` load and store), `resolve_field_expanded_offset()`
- [x] VM: new instruction handlers, expanded FB instance init for native FBs
- [x] `MemoryLayout::expanded_index()` / `expanded_len()` / `has_expanded_arrays()` helpers
- [x] DAP: array field expansion in variable viewer (shows `DO[0]`, `DO[1]`, ...)
- [x] PLC monitor: array field expansion in variable catalog and snapshot
- [x] Forced variables: uses `expanded_index()` for correct slot-to-value mapping
- [x] Modbus TCP device FB: `expand_registers()` for batched array I/O
- [x] Playground: `waveshare_8_relay.yaml` profile with `count: 8`, array access in demo

---

## Future Work

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
