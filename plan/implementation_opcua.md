# OPC-UA Server — Implementation Plan

> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.
> **Comm design:** [design_comm.md](design_comm.md) — communication layer architecture.
> **Deploy design:** [design_deploy.md](design_deploy.md) — remote deployment & agent architecture.
> **Target crate:** `crates/st-opcua-server/` (new)
> **Library:** `async-opcua` 0.18 — pure Rust (no OpenSSL), tokio-native, MPL-2.0

---

## Context

Our PLC runtime needs to expose its variables to HMI and SCADA systems via
OPC-UA. This is an **export/exposure layer** — it is NOT a CommDevice (field
device protocol). An OPC-UA server reads PLC state and lets external clients
browse, subscribe, and write PLC variables using the industry-standard OPC-UA
protocol.

The `RuntimeManager` in `st-target-agent` already exposes everything we need:
- `variable_catalog()` — all variable names + IEC types
- `all_variables()` — current values (as strings) + type + forced flag
- `subscribe_cycles()` — broadcast notification per scan cycle
- `force_variable()` / `unforce_variable()` — write path (async)

The integration goal is **zero changes to the scan cycle, VM, or CommManager**
and minimal, well-isolated additions to the target-agent.

---

## Architecture

```
Engine Thread (std::thread)           OPC-UA Server (tokio task)
────────────────────────              ──────────────────────────
scan_cycle()                          async-opcua ServerBuilder
  1. comm.read_inputs()               NodeManager with PLC nodes
  2. vm.scan_cycle("Main")
  3. comm.write_outputs()             ┌─────────────────────────┐
  4. snapshot → RuntimeState ──────── │ Value Update Task       │
     (Arc<RwLock>)                    │  reads all_variables()  │
                                      │  parses string→Variant  │
                                      │  sets node DataValues   │
                                      └────────────┬────────────┘
                                                   │
                                      ┌────────────▼────────────┐
  cmd_rx ◄── ForceVariable ◄──────── │ Write Handler           │
  (existing command channel)          │  OPC-UA Write service   │
                                      │  → force_variable()     │
                                      └─────────────────────────┘
```

**Read path** (HMI reads PLC variable):
1. Value update task polls `all_variables()` at configurable interval
2. Parses string values → OPC-UA Variants using type info
3. Updates node DataValues in the InMemoryNodeManager
4. OPC-UA subscriptions automatically publish changed values to clients

**Write path** (HMI writes PLC output):
1. OPC-UA client sends Write service call
2. Write handler extracts variable name from NodeId
3. Converts OPC-UA Variant → string value
4. Calls `provider.force_variable(name, value)` → existing RuntimeCommand path
5. Engine applies force on next scan cycle

---

## Threading Model & CPU Isolation

The PLC engine and the OPC-UA server run on **completely separate threads** and
do not interfere with each other, even under full CPU load.

```
┌─────────────────────────────────────────────────────────────────┐
│  Process: st-runtime                                            │
│                                                                 │
│  ┌──────────────────────────┐  ┌──────────────────────────────┐ │
│  │ Engine Thread            │  │ Tokio Runtime (multi-thread) │ │
│  │ (std::thread "plc-runtime") │  │ (worker pool, N cores)       │ │
│  │                          │  │                              │ │
│  │ loop {                   │  │  ┌─ HTTP API (axum)          │ │
│  │   read_inputs()          │  │  ├─ WebSocket monitor        │ │
│  │   scan_cycle()           │  │  ├─ DAP proxy                │ │
│  │   write_outputs()        │  │  ├─ OPC-UA server (NEW)      │ │
│  │   snapshot → state       │  │  │  ├─ accept connections    │ │
│  │   sleep(remaining)       │  │  │  ├─ value_sync task       │ │
│  │ }                        │  │  │  └─ subscription push     │ │
│  │                          │  │  └─ ...                      │ │
│  └──────────────────────────┘  └──────────────────────────────┘ │
│          ▲                              ▲                        │
│          │ NOT managed by tokio         │ All async I/O-bound    │
│          │ Dedicated OS thread          │ Never CPU-intensive    │
└─────────────────────────────────────────────────────────────────┘
```

**Why there's no interference:**

1. **The engine is a standalone `std::thread`**, spawned at
   `runtime_manager.rs:158`. It is NOT a tokio task. The OS scheduler gives it
   its own time slice independent of the tokio worker pool.

2. **All tokio tasks are I/O-bound.** The OPC-UA server's work is: accept TCP
   connections, read/write OPC-UA binary frames, parse a few hundred string
   values every 100ms, and push subscription notifications. None of this is
   CPU-intensive.

3. **The value sync task is lightweight.** Parsing ~200 string values to OPC-UA
   Variants every 100ms takes single-digit microseconds. The `RuntimeState`
   RwLock is held for reads only (no write contention with the engine's write
   side since the engine writes and OPC-UA reads).

4. **Even with a free-running engine (no cycle_time, spinning at max speed)**,
   the engine thread consumes one CPU core. The tokio runtime's worker threads
   use other cores for async I/O. On a multi-core system they don't compete.
   On a single-core system, the OS scheduler time-slices between the engine
   thread and tokio workers — the tokio workers use negligible CPU so the engine
   gets nearly 100% of its time slice.

5. **No architectural changes needed** for CPU isolation. The existing threading
   model already separates deterministic PLC execution from async I/O. The
   OPC-UA server is just another async I/O consumer in the tokio pool, like the
   HTTP server and WebSocket monitor that are already proven to work without
   affecting cycle timing.

---

## Deployment Model

The OPC-UA server is **embedded in the existing `st-runtime` binary** — it is
NOT a separate binary or a separate systemd service. It follows the same
pattern as all other runtime subsystems (HTTP API, WebSocket monitor, DAP
proxy): spawned as a tokio task inside the agent process.

### How it fits the existing deployment

```
Developer Machine                              Target Device
┌────────────────────────┐                    ┌──────────────────────────────────┐
│                        │                    │                                  │
│  st-cli deploy ────────┼──── SSH/HTTP ─────►│  st-runtime (single binary)      │
│                        │                    │  ├── HTTP REST API    (:4840)    │
│  st-cli target         │                    │  ├── DAP proxy        (:4841)    │
│    bootstrap           │                    │  ├── OPC-UA server    (:4842)    │  ◄── NEW
│                        │                    │  ├── WebSocket monitor            │
│                        │                    │  ├── Runtime Manager              │
│                        │                    │  │   └── Engine thread            │
│                        │                    │  └── Comm Manager                 │
│                        │                    │                                  │
│                        │                    │  Config: /etc/st-plc/agent.yaml  │
│                        │                    │  systemd: st-runtime.service     │
└────────────────────────┘                    └──────────────────────────────────┘
                                                       ▲           ▲
                                                       │           │
                                                  HMI/SCADA    Field devices
                                                (OPC-UA client) (Modbus, etc.)
```

### Key deployment facts

1. **No separate service** — the OPC-UA server is a tokio task spawned inside
   the agent process, alongside the HTTP server and monitor. One process, one
   systemd unit, one binary.

2. **No separate binary** — `st-runtime` is still the only binary deployed to
   the target. The OPC-UA feature is compiled in (feature-gated via `opcua`
   cargo feature).

3. **Same bootstrap process** — `st-cli target bootstrap user@host` deploys the
   `st-runtime` binary (which now includes OPC-UA) via SSH/SCP and installs the
   same `st-runtime.service` systemd unit. No new installation steps.

4. **Config-driven activation** — the OPC-UA server starts only when
   `opcua_server.enabled: true` in `agent.yaml`. Existing deployments without
   this section continue to work unchanged (serde defaults to `enabled: false`).

5. **Same lifecycle** — the OPC-UA server starts with the agent, stays up
   across program start/stop/reload cycles, and shuts down with the agent.
   When no program is running, the OPC-UA address space is empty; when a
   program starts, nodes are created from the variable catalog.

6. **Port allocation** — HTTP API on 4840, DAP on 4841, OPC-UA on 4842.
   All configurable via `agent.yaml`, all behind the same `network.bind` address.

### Deploying with OPC-UA enabled

```yaml
# /etc/st-plc/agent.yaml on the target
agent:
  name: line1-plc

network:
  bind: 0.0.0.0
  port: 4840

opcua_server:
  enabled: true
  port: 4842
  security_policy: None
  anonymous_access: true
```

After deploying the binary and config, restart the service:
```bash
sudo systemctl restart st-runtime
```

The OPC-UA server immediately begins listening on `opc.tcp://0.0.0.0:4842`.
HMI tools (FUXA, Ignition, WinCC OA, etc.) discover it via the endpoint URL.

### Build variants

| Build | Command | OPC-UA included | Binary size impact |
|-------|---------|-----------------|-------------------|
| Default | `cargo build --release` | Yes (default feature) | +async-opcua + crypto |
| Without OPC-UA | `cargo build --release --no-default-features` | No | Same as today |
| musl-static | `cargo build --release --target x86_64-unknown-linux-musl` | Yes | Pure Rust, no OpenSSL |

The `opcua` feature is enabled by default. For size-constrained targets that
don't need OPC-UA, build without it.

---

## New Crate: `crates/st-opcua-server/`

### Dependencies

```toml
[dependencies]
async-opcua-server = "0.18"
async-opcua-types = "0.18"
async-opcua-crypto = "0.18"
async-opcua-nodes = "0.18"
tokio = { version = "1", features = ["rt", "time", "sync"] }
tracing = "0.1"
serde = { version = "1", features = ["derive"] }
```

No dependency on `st-engine`, `st-target-agent`, or `st-ir`. The crate is
decoupled via a trait.

### File Structure

```
crates/st-opcua-server/
  Cargo.toml
  src/
    lib.rs               — PlcDataProvider trait, OpcuaServerConfig, public API
    config.rs            — Configuration types + defaults
    type_map.rs          — IEC type→OPC-UA type mapping, value string parsers
    address_space.rs     — Build/rebuild OPC-UA node hierarchy from catalog
    value_sync.rs        — Background task: poll provider → update nodes
    write_handler.rs     — OPC-UA Write → force_variable bridge
```

### PlcDataProvider Trait

```rust
/// Abstraction over the PLC runtime's variable interface.
/// Implemented by st-target-agent wrapping RuntimeManager.
#[async_trait]
pub trait PlcDataProvider: Send + Sync + 'static {
    fn variable_catalog(&self) -> Vec<CatalogEntry>;
    fn all_variables(&self) -> Vec<VariableSnapshot>;
    fn runtime_status(&self) -> String;
    fn cycle_stats(&self) -> Option<CycleStatsSnapshot>;
    async fn force_variable(&self, name: &str, value: &str) -> Result<String, String>;
    async fn unforce_variable(&self, name: &str) -> Result<(), String>;
}
```

This trait lives in `st-opcua-server` and is implemented in `st-target-agent`.

### IEC 61131-3 → OPC-UA Type Mapping

| IEC Type | OPC-UA DataType | Value Parse |
|----------|-----------------|-------------|
| BOOL | Boolean | "TRUE"/"FALSE" → bool |
| SINT | SByte (i8) | parse i64, cast i8 |
| INT | Int16 (i16) | parse i64, cast i16 |
| DINT | Int32 (i32) | parse i64, cast i32 |
| LINT | Int64 (i64) | parse i64 |
| USINT | Byte (u8) | parse u64, cast u8 |
| UINT | UInt16 (u16) | parse u64, cast u16 |
| UDINT | UInt32 (u32) | parse u64, cast u32 |
| ULINT | UInt64 (u64) | parse u64 |
| REAL | Float (f32) | parse f64, cast f32 |
| LREAL | Double (f64) | parse f64 |
| STRING | String | strip surrounding quotes |
| TIME | Int64 (ms) | parse "T#..." literal |

Value string format follows `st-engine/src/debug.rs:format_value()`:
- Bool: `"TRUE"` / `"FALSE"`
- Int/UInt: decimal string `"42"`, `"-7"`
- Real: `"{:.6}"` format `"3.140000"`
- String: `"'hello'"` (single-quoted)
- Time: `"T#500ms"`, `"T#1s500ms"`

### OPC-UA Address Space

```
Objects (i=85)
  └── PLCRuntime  (ns=2;s=PLCRuntime)              FolderType
       ├── Status      (ns=2;s=_status)             String: "Running"/"Idle"/"Error"
       ├── CycleCount  (ns=2;s=_cycle_count)        UInt64
       ├── CycleTimeUs (ns=2;s=_cycle_time_us)      UInt64
       ├── Globals     (ns=2;s=Globals)              FolderType
       │    ├── io_rack_DI_0  (ns=2;s=io_rack_DI_0)
       │    ├── io_rack_AI_0  (ns=2;s=io_rack_AI_0)
       │    └── pump_vfd_SPEED_REF (ns=2;s=pump_vfd_SPEED_REF)
       └── Programs    (ns=2;s=Programs)             FolderType
            └── Main   (ns=2;s=Main)                 FolderType
                 ├── counter      (ns=2;s=Main.counter)
                 └── fb_instance  (ns=2;s=Main.fb_instance)   FolderType
                      ├── field1  (ns=2;s=Main.fb_instance.field1)
                      └── field2  (ns=2;s=Main.fb_instance.field2)
```

**NodeId scheme:** String identifiers in namespace 2, using the exact variable
name from the catalog. Variables without dots go under "Globals"; variables
with dots (e.g., `Main.counter`) get intermediate folders built from the dot
segments.

**Access levels:**
- All variables: readable (OPC-UA Read service, subscriptions)
- All variables: writable (maps to force_variable — same semantics as monitor)
- Forced status exposed via a custom property on each variable node

### Configuration

Added to `agent.yaml` (deployment-specific):

```yaml
opcua_server:
  enabled: true               # on by default — core feature for HMI/SCADA
  port: 4842                  # default: agent HTTP port + 2
  security_policy: None       # None | Basic256Sha256 | Aes256Sha256RsaPss
  message_security_mode: None # None | Sign | SignAndEncrypt
  anonymous_access: true
  application_name: "ST-PLC OPC-UA Server"
  user_tokens: []             # [{username: "admin", password: "..."}]
  sampling_interval_ms: 100   # how often to sync values from engine
```

Port 4842 chosen to avoid conflict with HTTP (4840) and DAP (4841).

---

## Changes to Existing Code

### 1. `crates/st-target-agent/src/config.rs` — ADD section

Add `OpcuaServerConfig` struct and field to `AgentConfig`:

```rust
// config.rs — new field in AgentConfig
#[serde(default)]
pub opcua_server: OpcuaServerConfig,
```

New struct with serde defaults (backward-compatible — existing agent.yaml
files parse fine without the section):

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct OpcuaServerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_opcua_port")]
    pub port: u16,
    #[serde(default = "default_security_policy")]
    pub security_policy: String,
    #[serde(default)]
    pub anonymous_access: bool,  // uses existing default_true()
    #[serde(default = "default_sampling_interval")]
    pub sampling_interval_ms: u64,
}
```

**Impact:** New code only. Existing config parsing unaffected (serde default).

### 2. `crates/st-target-agent/src/server.rs` — ADD wiring

In `build_app_state()`, after building AppState, if `config.opcua_server.enabled`,
spawn the OPC-UA server:

```rust
// server.rs — after Arc::new(AppState { ... })
if state.config.opcua_server.enabled {
    let provider = AgentDataProvider::new(Arc::clone(&state));
    let opcua_config = /* map from AgentConfig.opcua_server */;
    tokio::spawn(st_opcua_server::run_opcua_server(opcua_config, Arc::new(provider)));
}
```

The `AgentDataProvider` struct (new, in a new file `src/opcua_bridge.rs`)
implements `PlcDataProvider` by delegating to `state.runtime_manager`:

```rust
struct AgentDataProvider { state: Arc<AppState> }

impl PlcDataProvider for AgentDataProvider {
    fn variable_catalog(&self) -> Vec<CatalogEntry> {
        self.state.runtime_manager.variable_catalog()
        // map CatalogEntry types between crates
    }
    fn all_variables(&self) -> Vec<VariableSnapshot> {
        self.state.runtime_manager.all_variables()
    }
    async fn force_variable(&self, name: &str, value: &str) -> Result<String, String> {
        self.state.runtime_manager.force_variable(name.to_string(), value.to_string())
            .await
            .map_err(|e| e.to_string())
    }
    // ... etc
}
```

**Impact:** New file + ~10 lines in server.rs. No changes to existing functions.

### 3. `crates/st-target-agent/Cargo.toml` — ADD dependency

```toml
st-opcua-server = { path = "../st-opcua-server", optional = true }

[features]
default = ["opcua"]
opcua = ["st-opcua-server"]
```

Feature-gated so builds without OPC-UA produce the same binary as today.

### 4. Root `Cargo.toml` — ADD workspace member

```toml
members = [
    # ... existing ...
    "crates/st-opcua-server",
]
```

### Summary of existing file changes

| File | Change | Lines |
|------|--------|-------|
| `crates/st-target-agent/src/config.rs` | Add OpcuaServerConfig struct + field | ~25 new |
| `crates/st-target-agent/src/server.rs` | Spawn OPC-UA server if enabled | ~10 new |
| `crates/st-target-agent/src/opcua_bridge.rs` | **NEW FILE** — PlcDataProvider impl | ~60 new |
| `crates/st-target-agent/Cargo.toml` | Add optional dependency + feature | ~3 new |
| `Cargo.toml` (root) | Add workspace member | 1 new |

**Zero changes to:**
- `st-engine` (scan cycle, VM, CommManager)
- `st-ir` (Value, Module, types)
- `st-comm-api` (traits, profiles, codegen)
- `st-comm-sim` (simulated devices)
- `st-monitor` (WebSocket monitor)
- `runtime_manager.rs` (engine thread, cycle loop, state management)

---

## Logging Requirements

The OPC-UA server must produce thorough log output so operators can diagnose
connection issues, subscription behavior, and data flow problems. All logging
uses the `tracing` crate (consistent with the rest of the runtime).

### Server lifecycle (INFO)

```
INFO  OPC-UA server starting on opc.tcp://0.0.0.0:4842
INFO  OPC-UA server: security policy=None, anonymous=true
INFO  OPC-UA server: endpoint registered — opc.tcp://0.0.0.0:4842/opcua
INFO  OPC-UA server ready, waiting for connections
INFO  OPC-UA server shutting down
```

### Client connections (INFO)

```
INFO  OPC-UA: client connected from 192.168.1.50:54321 (session=1)
INFO  OPC-UA: client authenticated — anonymous (session=1)
INFO  OPC-UA: client disconnected (session=1, reason=closed by client)
INFO  OPC-UA: client connection lost (session=1, reason=timeout after 5000ms)
```

### Address space (INFO on rebuild, DEBUG on individual nodes)

```
INFO  OPC-UA: building address space from 47 variables (catalog version 1)
INFO  OPC-UA: address space ready — 47 variable nodes, 12 folder nodes
INFO  OPC-UA: catalog changed (version 1 → 2), rebuilding address space
INFO  OPC-UA: address space cleared — runtime stopped, 0 variables
DEBUG OPC-UA: added node ns=2;s=io_rack_DI_0 (BOOL, read/write)
DEBUG OPC-UA: added folder ns=2;s=Main
DEBUG OPC-UA: added node ns=2;s=Main.counter (INT, read/write)
```

### Value sync (DEBUG, periodic summary at INFO)

```
DEBUG OPC-UA: value sync — updated 47/47 variables in 15µs
INFO  OPC-UA: value sync stats — 1000 cycles, avg 12µs, max 45µs
WARN  OPC-UA: value sync — variable 'io_rack_AI_0' parse error: "not_a_number" as INT
```

### Subscriptions (INFO on create/delete, DEBUG on publish)

```
INFO  OPC-UA: subscription created (session=1, sub_id=1, interval=250ms, 5 items)
INFO  OPC-UA: monitored item added — ns=2;s=io_rack_DI_0 (session=1, sub_id=1)
INFO  OPC-UA: subscription deleted (session=1, sub_id=1)
DEBUG OPC-UA: publish (session=1, sub_id=1) — 3 changed values
```

### Write operations (INFO always — these are control actions)

```
INFO  OPC-UA: write request (session=1) — ns=2;s=pump_vfd_SPEED_REF = 45.0 (Double)
INFO  OPC-UA: write applied — forced pump_vfd_SPEED_REF = 45.000000
WARN  OPC-UA: write rejected — ns=2;s=unknown_var — BadNodeIdUnknown
WARN  OPC-UA: write failed — ns=2;s=io_rack_DI_0 — force_variable error: "Runtime is not running"
```

### Error conditions (WARN/ERROR)

```
WARN  OPC-UA: value sync skipped — runtime not running
WARN  OPC-UA: client connection rejected — max sessions (10) reached
ERROR OPC-UA: server failed to bind on 0.0.0.0:4842 — address already in use
ERROR OPC-UA: value sync panic — restarting task
```

---

## Implementation Phases

### Phase 1: Type mapping + address space (no integration)

- [x] Create `crates/st-opcua-server/` with Cargo.toml
- [x] Define `PlcDataProvider` trait + config types in `lib.rs`
- [x] Implement `type_map.rs` — IEC→OPC-UA type mapping, string→Variant parser,
  Variant→string serializer (for writes). Unit tests for every IEC type.
- [x] Implement `address_space.rs` — build OPC-UA node hierarchy from a catalog,
  handle dot-separated names, create folders. Unit test with mock catalog.

### Phase 2: Server core (no integration)

- [x] Implement `value_sync.rs` — background task that polls PlcDataProvider,
  parses values, updates node DataValues. Respects `sampling_interval_ms`.
- [x] Implement `write_handler.rs` — OPC-UA Write callback, Variant→string,
  call `force_variable`.
- [x] Implement `run_opcua_server()` — ServerBuilder setup, NodeManager creation,
  endpoint configuration, security policy selection, task spawning.
- [x] Integration test with a mock PlcDataProvider that exposes canned variables.

### Phase 3: Agent integration (minimal existing changes)

- [x] Add `OpcuaServerConfig` to `st-target-agent/src/config.rs`
- [x] Create `st-target-agent/src/opcua_bridge.rs` — PlcDataProvider impl
- [x] Wire up in `server.rs` — spawn OPC-UA server when enabled
- [x] Add workspace member, feature flags, dependencies
- [ ] Test: start agent with `opcua_server.enabled: true`, connect with
  UaExpert or opcua-client-cli, browse address space, read values

### Phase 4: Catalog rebuild + robustness

- [x] Detect catalog changes (compare previous catalog to current on each poll)
- [x] Rebuild address space when catalog changes (program start/stop/online change)
- [x] Handle runtime status transitions gracefully (server stays up, nodes cleared)
- [x] Logging: connection events, subscription activity, write attempts

### Phase 5: End-to-end echo test + playground example

- [x] Create `playground/opcua_echo/` with project files (see E2E test below)
- [x] Create `crates/st-target-agent/tests/opcua_echo.rs` — full round-trip test
- [x] Add `async-opcua-client` as a dev-dependency for the test

---

## Verification

### Unit tests (in st-opcua-server)
- `type_map.rs`: round-trip every IEC type (string→Variant→string)
- `address_space.rs`: flat globals, dotted hierarchical names, empty catalog
- `value_sync.rs`: mock provider, verify nodes updated after poll

### Integration test
- Compile a test ST program with globals + PROGRAM locals
- Start RuntimeManager with the program
- Start OPC-UA server with mock AgentDataProvider
- Connect with `async-opcua-client`, browse, read, write, subscribe

### End-to-end echo test

Full round-trip through the OPC-UA server, PLC engine, and OPC-UA client
proving data flows correctly in both directions.

**Playground project: `playground/opcua_echo/`**

```
playground/opcua_echo/
  plc-project.yaml
  main.st
```

`plc-project.yaml`:
```yaml
name: OpcUaEcho
entryPoint: Main
engine:
  cycle_time: 10ms
```

`main.st` — the PLC program echoes a written value to a third variable:
```st
VAR_GLOBAL
    source_val   : INT := 42;      (* initial value the client reads *)
    written_val  : INT := 0;       (* client writes here via OPC-UA *)
    echo_val     : INT := 0;       (* PLC copies written_val here *)
    bool_source  : BOOL := TRUE;
    bool_written : BOOL := FALSE;
    bool_echo    : BOOL := FALSE;
    real_source  : REAL := 3.14;
    real_written : REAL := 0.0;
    real_echo    : REAL := 0.0;
END_VAR

PROGRAM Main
    (* Echo: copy written values to echo variables *)
    echo_val    := written_val;
    bool_echo   := bool_written;
    real_echo   := real_written;
END_PROGRAM
```

**Test sequence** (`e2e_echo.rs`):

```
 OPC-UA Client              OPC-UA Server              PLC Engine
 ────────────               ──────────────             ──────────
 1. Connect to server       Listening on 4842
                                                       Running Main

 2. Browse address space
    → verify source_val,
      written_val, echo_val
      all visible

 3. Read source_val
    → assert value == 42

 4. Write 42 to written_val
    (via OPC-UA Write)      → force_variable            → applies force
                            ("written_val", "42")         next cycle

 5. Wait ~200ms             value_sync polls             Main executes:
    (2+ scan cycles)        → updates nodes              echo_val := written_val

 6. Read echo_val
    → assert value == 42    ← node DataValue == 42

 7. Repeat with BOOL:
    Read bool_source → TRUE
    Write TRUE to bool_written
    Wait → Read bool_echo → assert TRUE

 8. Repeat with REAL:
    Read real_source → 3.14
    Write 3.14 to real_written
    Wait → Read real_echo → assert ≈ 3.14

 9. Write 0 to written_val
    Wait → Read echo_val → assert 0
    (proves the value flows, not just initial state)

10. Disconnect
```

**Implementation** (`crates/st-opcua-server/tests/e2e_echo.rs`):

Uses `async-opcua-client` as a dev-dependency. The test:
1. Compiles the echo ST program (using `st-syntax`, `st-compiler`)
2. Starts a `RuntimeManager` with the compiled module
3. Starts the OPC-UA server with the `AgentDataProvider` bridge
4. Creates an `async-opcua` client, connects to `opc.tcp://localhost:4842`
5. Runs the 10-step sequence above
6. Asserts at each step
7. Shuts down cleanly

This tests the full stack: ST compilation → VM execution → RuntimeState
snapshot → OPC-UA value sync → OPC-UA client Read, and the reverse:
OPC-UA client Write → force_variable → engine applies → VM executes →
snapshot → OPC-UA Read verification.

Covers three IEC types (INT, BOOL, REAL) and proves bidirectional data
flow through the complete system.

### Manual verification
- Start full agent with `opcua_server.enabled: true`
- Connect with UaExpert (free OPC-UA test client) or Prosys OPC UA Browser
- Verify: browse tree matches PLC variables, values update live,
  writing an output variable forces it in the PLC

### Build verification
- `cargo build --release --target x86_64-unknown-linux-musl` — static binary
- `cargo build --no-default-features` (without opcua feature) — same size as before
- `cargo clippy -- -D warnings` — zero warnings