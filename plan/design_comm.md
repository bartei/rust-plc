# Communication Layer — Design Document

> **Progress tracker:** [implementation_comm.md](implementation_comm.md)
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.

## Overview

The communication layer provides users with access to external devices (I/O racks, VFDs, sensors)
through standard IEC 61131-3 function block syntax. Device types are derived from YAML profiles
and exposed as callable function blocks in ST code.

```st
PROGRAM Main
VAR
    io_rack : Sim8DI4AI4DO2AO;
    pump_vfd : SimVfd;
END_VAR
    io_rack(refresh_rate := T#10ms);
    pump_vfd(refresh_rate := T#10ms);

    io_rack.DO_0 := io_rack.DI_0;

    IF io_rack.DI_6 AND pump_vfd.READY THEN
        pump_vfd.RUN := TRUE;
    END_IF;

    pump_vfd.SPEED_REF := INT_TO_REAL(IN1 := io_rack.AI_3) * 0.005;
END_PROGRAM
```

## Architecture

### Native Function Blocks (NativeFb)

Communication devices are implemented as **native function blocks** — Rust-backed FBs that appear
as normal `FUNCTION_BLOCK` types in the editor and debugger but execute native Rust code instead
of interpreted ST instructions.

The core trait:

```rust
pub trait NativeFb: Send + Sync {
    fn type_name(&self) -> &str;
    fn layout(&self) -> &NativeFbLayout;
    fn execute(&self, fields: &mut [Value]);
}
```

`NativeFbLayout` is the **single source of truth** for all tooling:

| Consumer | What it reads |
|----------|---------------|
| **Semantic analyzer** | Field names + types → `SymbolKind::FunctionBlock { params, outputs }` |
| **Compiler** | `MemoryLayout` → synthetic `Function` entry with correct locals |
| **VM** | Field slice → `execute()` dispatch for empty-body FBs |
| **LSP** | Same symbol table → dot-completion, hover, type checking |
| **DAP** | Same `Function.locals` → variable expansion, watch expressions |

### NativeFbRegistry

A central registry holds all available native FB types:

```rust
pub struct NativeFbRegistry {
    entries: Vec<Box<dyn NativeFb>>,
}
```

Built at startup from device profiles. Passed through the pipeline:
1. `analyze_with_native_fbs()` — injects FB types into symbol table
2. `compile_with_native_fbs()` — creates synthetic Function entries in Module
3. `Vm::new_with_native_fbs()` — dispatches `CallFb` to `execute()` for native FBs

### Two-Layer Model

- **Links** — Physical transport (serial, TCP, simulated). Planned as native FBs.
- **Devices** — Protocol endpoints parameterized by YAML profiles. Currently simulated.

Link-less devices (e.g., Ethernet/IP) take connection params directly as VAR_INPUT.

## Device Profiles

YAML files define the register map and field schema for specific hardware:

```yaml
name: Sim8DI4AI4DO2AO
vendor: Simulated
protocol: simulated
fields:
  - name: DI_0
    type: BOOL
    direction: input
    register: { address: 0, kind: virtual }
  - name: DO_0
    type: BOOL
    direction: output
    register: { address: 20, kind: virtual }
```

`DeviceProfile::to_native_fb_layout()` converts a profile into a `NativeFbLayout` with:
- `refresh_rate : TIME` (VarInput — configuration parameter)
- Diagnostic fields: `connected`, `error_code`, `io_cycles`, `last_response_ms` (Var)
- All profile I/O fields (Var — readable and writable from ST code)

### Field Mapping

| Profile direction | FB var kind | ST access |
|-------------------|-------------|-----------|
| input | Var | Read via dot notation (`dev.DI_0`) |
| output | Var | Read/write via dot notation (`dev.DO_0 := TRUE`) |
| inout | Var | Read/write via dot notation |

All I/O fields use Var (not VarOutput) so the user program can both read and write them.

## Simulated Device

`SimulatedNativeFb` wraps a `SimulatedDevice` with an `Arc<Mutex<HashMap<String, IoValue>>>` state
that is shared with the web UI. The `execute()` method:

1. Reads input-direction fields from shared state → writes to FB field slots
2. Reads output-direction fields from FB field slots → writes to shared state
3. Updates diagnostic fields (connected, io_cycles, etc.)

The web UI (HTTP + WebSocket) allows manual toggle of inputs and observation of outputs.

## Compile Pipeline Integration

```
Profile YAML → NativeFbLayout → NativeFbRegistry
                                      ↓
                              analyze_with_native_fbs()
                                      ↓
                              compile_with_native_fbs()
                                      ↓
                              Vm::new_with_native_fbs()
                                      ↓
                              call_fb() → NativeFb::execute()
```

At `call_fb()`, the VM checks `module.native_fb_indices`. If the function is a native FB:
1. Load/create instance state from `fb_instances` (same as normal FBs)
2. Apply input arguments (same as normal FBs)
3. Call `native_fb.execute(&mut locals)` (instead of pushing a call frame)
4. Save instance state back

This means native FB instances persist state across cycles, support field access, and appear
in the debugger's variable view — all using the same mechanisms as user-defined FBs.

## Plugin System (Planned)

**Tier 1 — Device profile plugins:**
- Git repos containing YAML profiles + optional ST library code
- Referenced in `plc-project.yaml` under a `plugins:` section
- Managed via `st-cli plugin fetch/update/list`
- No binary recompilation needed

**Tier 2 — Protocol plugins:**
- New protocols (Modbus, PROFINET, etc.) require Rust implementation
- Contributed to the core project as workspace crates
- Each protocol is a generic NativeFb parameterized by the profile

## Connection Lifecycle

- **First call:** Parameters latched, connection opened
- **Subsequent calls:** Idempotent on config; perform scheduled I/O
- **Connection loss:** `connected := FALSE`, retry with backoff
- **Refresh rate:** Handled inside `execute()` with internal timing

## Multi-Rate I/O

The `refresh_rate` parameter controls how often the device performs actual I/O.
Between updates, field values hold their last-known values. This replaces the
old `CommManager` multi-rate scheduling with a simpler per-device mechanism.

## Future Work

- Real protocol implementations (Modbus RTU/TCP, serial/TCP links)
- Plugin CLI (`st-cli plugin fetch/update/list`)
- WASM protocol plugins for third-party proprietary protocols
- OPC-UA server integration with native FB field access
