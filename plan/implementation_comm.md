# Communication Layer — Progress Tracker

> **Design document:** [design_comm.md](design_comm.md) — architecture, examples, rationale.
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.
> **See also:** [implementation_native.md](implementation_native.md) — native compilation.
> **See also:** [implementation_deploy.md](implementation_deploy.md) — remote deployment (Phase 15).

---

## Core API + Simulated Device

### st-comm-api crate

- [x] `CommLink` trait (open, close, send, receive, diagnostics)
- [x] `CommDevice` trait (configure, bind_link, read_inputs, write_outputs, acyclic)
- [x] `DeviceProfile` struct + `ProfileField` with register mappings
- [x] `CommError`, `LinkDiagnostics`, `DeviceDiagnostics` types
- [x] `AcyclicRequest` / `AcyclicResponse` types
- [x] Device profile YAML parser
- [x] Profile-to-ST code generator (flat `{device}_{field}` globals)
- [x] Project YAML parser (`links:` + `devices:` sections)
- [x] `write_io_map_file()` — writes `_io_map.st` only if changed

### st-comm-sim crate

- [x] `CommDevice` impl with in-memory register storage
- [x] Simulated link (no network)
- [x] Web UI server (HTTP + JSON polling, per-device port)
- [x] Toggle digital inputs, set analog inputs via web UI
- [x] Display digital/analog outputs via web UI
- [ ] Show device diagnostics in web UI (connected, cycle count, last update)
- [x] Loads standard device profile YAML
- [x] Multiple simulated devices per project
- [x] Unit tests: register read/write, profile loading, I/O direction enforcement
- [x] Integration test: full scan cycle with simulated device

### Communication Manager

- [x] Parse `links:` / `devices:` from plc-project.yaml
- [x] Create and register device instances
- [ ] Coordinate bus access for shared links (mutex/queue)
- [x] Scan cycle integration: `read_inputs` → execute → `write_outputs`
- [x] Field ↔ VM global mapping via `{device}_{field}` slots
- [x] Direction-aware I/O (input fields read-only, output fields write-only)
- [x] Multi-rate scheduling: per-device `cycle_time` with independent timers
- [ ] Auto-generate `CommDiag` fields per device
- [ ] Connection monitoring + automatic reconnection with backoff
- [ ] Diagnostics exposed via monitor server

### Engine + CLI + DAP integration

- [x] `Engine` owns `CommManager`, calls `read_inputs`/`write_outputs` per scan
- [x] `Engine::register_comm_device()` helper
- [x] `Vm::set_global_by_slot` / `get_global_by_slot` for fast slot-based I/O
- [x] `st-cli run` loads config, regenerates `_io_map.st`, starts web UIs
- [x] `st-cli check` regenerates `_io_map.st`
- [x] `st-cli comm-gen [path]` for explicit regeneration
- [x] DAP server does the same setup before launch
- [x] `read_inputs`/`write_outputs` called at every DAP scan-cycle boundary
- [ ] `st-cli comm-status` — link health and device connection state
- [ ] `st-cli profile validate` — check device profile YAML for errors

### On-disk symbol map

- [x] `_io_map.st` written to project root, regenerated only when changed
- [x] File is gitignored
- [x] Human-readable header per device + column-aligned mapping table
- [x] Picked up by project autodiscovery (LSP, semantic checker, compiler, runtime, DAP)

### Bundled device profiles

- [x] `sim_8di_4ai_4do_2ao` — 8 DI, 4 AI, 4 DO, 2 AO
- [ ] `sim_16di_16do` — 16-channel digital I/O
- [x] `sim_vfd` — simulated VFD

### Playground + Docs

- [x] `playground/sim_project/` end-to-end example
- [ ] Simulated device quickstart doc
- [ ] "How to create a device profile" doc

---

## Diagnostics Exposure

### Layer 1 — ST globals (ground truth)

- [ ] Reserve six diag globals per device at `register_device()`
- [ ] Write diag values after `write_outputs()` each cycle
- [ ] Emit `--- DIAGNOSTICS ---` block in `_io_map.st`
- [ ] Link diagnostics globals
- [ ] Engine-level globals (`engine_cycle_count`, `engine_*_cycle_us`)
- [ ] Unit test: globals exist, get updated, readable from ST code

### Layer 2 — HTTP JSON endpoint

- [ ] `GET /api/diagnostics` — full snapshot with `"schema": "1"`
- [ ] `GET /api/diagnostics/devices/{name}` — single device
- [ ] `GET /api/diagnostics/summary` — healthy/count/connected/errors
- [ ] Separate port from monitor WebSocket, configured in plc-project.yaml
- [ ] Read-only, no auth in v1, bind `127.0.0.1` by default

### Layer 3 — Documentation

- [ ] Field-by-field diag reference (units, semantics, timing)
- [ ] ST code example: alarm + watchdog using `*_diag_connected`
- [ ] `/api/diagnostics` schema reference + versioning policy
- [ ] Node-RED quickstart (inject → http request → json → switch)
- [ ] FUXA quickstart (Web API device + tag bindings)
- [ ] Cross-link from quickstart docs

---

## Real Protocol Implementations

### st-comm-link-tcp

- [ ] TCP socket management (connect, reconnect, timeout)
- [ ] Implements `CommLink` trait
- [ ] Unit tests with mock TCP listener

### st-comm-link-serial

- [ ] Serial port management (RS-485/RS-232, baud, parity, data/stop bits)
- [ ] Implements `CommLink` trait
- [ ] Unit tests with mock serial port / PTY pair

### st-comm-modbus

- [ ] Implements `CommDevice` trait for Modbus
- [ ] TCP framing: MBAP header (auto-selected for TCP links)
- [ ] RTU framing: CRC-16, silence detection (auto-selected for serial links)
- [ ] ASCII framing: LRC (optional, for serial links)
- [ ] Read coils, discrete inputs, holding registers, input registers
- [ ] Write single/multiple coils, single/multiple registers
- [ ] Cyclic polling with configurable interval
- [ ] Device profile field ↔ register mapping
- [ ] Unit tests with mock link
- [ ] Integration tests with Modbus simulator

### Additional CLI commands

- [ ] `st-cli comm-test` — send test read to verify connectivity
- [ ] `st-cli profile import` — convert GSD/GSDML/ESI/EDS → YAML profile

### Bundled hardware device profiles

- [ ] Generic Modbus I/O (8/16/32 channel variants)
- [ ] ABB ACS580 VFD
- [ ] Siemens G120 VFD
- [ ] WAGO 750-352 I/O coupler
- [ ] Generic temperature sensor (RTD/thermocouple)

### Documentation

- [ ] Communication architecture guide
- [ ] "Creating a Link Extension" tutorial
- [ ] "Creating a Device Extension" tutorial
- [ ] Modbus quickstart (TCP + RTU examples)

---

## Future Protocol Extensions

- [ ] `st-comm-link-udp` — UDP link
- [ ] `st-comm-profinet` — PROFINET I/O
- [ ] `st-comm-ethercat` — EtherCAT
- [ ] `st-comm-canopen` — CANopen / CAN bus
- [ ] `st-comm-opcua` — OPC UA client
- [ ] `st-comm-mqtt` — MQTT publish/subscribe
- [ ] `st-comm-s7` — Siemens S7 protocol
- [ ] `st-comm-ethernet-ip` — EtherNet/IP (Allen-Bradley)