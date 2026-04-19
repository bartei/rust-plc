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

## Cleanup (Remaining)

- [ ] Remove `_io_map.st` codegen path (`st-comm-api/src/codegen.rs`)
- [ ] Remove `CommManager` (`st-engine/src/comm_manager.rs`)
- [ ] Remove `CommConfig`/`LinkConfig` from `st-comm-api/src/config.rs`
- [ ] Remove `links:`/`devices:` parsing from CLI comm_setup
- [ ] Remove `_io_map.st` from bundle system
- [ ] Generate stub `.st` files for go-to-definition on native FB types

---

## Future Work

### Real Protocol Implementations

- [ ] `SerialLinkFb` — wraps serial port, NativeFb with port/baud/parity params
- [ ] `TcpLinkFb` — wraps TCP socket, NativeFb with host/port params
- [ ] `ModbusRtuDeviceFb` — generic Modbus RTU, parameterized by profile
- [ ] `ModbusTcpDeviceFb` — generic Modbus TCP, parameterized by profile

### Plugin System

- [ ] `plugin.yaml` schema definition
- [ ] `plugins:` section in `plc-project.yaml`
- [ ] `st-cli plugin fetch` — clone/update git repos
- [ ] `st-cli plugin list/update/info` commands
- [ ] `.st-plugins.lock` lockfile for reproducible builds
- [ ] Plugin profiles included in bundle deployment

### Advanced Features

- [ ] WASM protocol plugins (Tier 3)
- [ ] Global FB instances (for multi-program device sharing)
- [ ] Online change with native FB state migration
