# st-opcua-server

OPC-UA server for HMI/SCADA integration with the PLC runtime.

## Purpose

Exposes all PLC variables to OPC-UA clients (Ignition, FUXA, WinCC OA, UaExpert, Node-RED, Grafana, custom applications). Any OPC-UA compliant tool can browse, subscribe to, and write PLC variables using the industry-standard OPC-UA protocol.

This is an **export layer** — it reads PLC state and allows external clients to interact with it. It is NOT a field device protocol (that's `st-comm-api`).

## How to Use

### Connecting

The OPC-UA server is enabled by default on port 4842. Point any client to:

```
opc.tcp://<target-ip>:4842
```

No configuration needed for basic anonymous access.

### Browsing Variables

The address space mirrors PLC variables:

```
Objects
  └── PLCRuntime
       ├── Status            "Running"
       ├── Cycle Count       15432
       ├── Cycle Time (us)   12
       ├── Globals
       │    ├── io_rack_DI_0       TRUE
       │    └── pump_vfd_SPEED_REF 45.0
       └── Programs
            └── Main
                 ├── counter        42
                 └── pid_ctrl
                      └── output    47.3
```

### Writing Variables

OPC-UA writes force the variable in the PLC engine — the value persists until unforced. This is the correct behavior for HMI control (setpoints, commands).

### NodeId Scheme

All variables use string NodeIds in namespace 2:
- `ns=2;s=io_rack_DI_0` — flat global
- `ns=2;s=Main.counter` — program local
- `ns=2;s=Main.pid_ctrl.output` — FB instance field

## Configuration

In `agent.yaml`:

```yaml
opcua_server:
  enabled: true                  # default: true
  port: 4842                     # default: 4842
  security_policy: None          # None | Basic256Sha256 | Aes256Sha256RsaPss
  anonymous_access: true         # default: true
  sampling_interval_ms: 100      # value sync rate (default: 100ms)
```

## Public API

### PlcDataProvider Trait

The crate is decoupled from the PLC engine via this trait:

```rust
#[async_trait]
pub trait PlcDataProvider: Send + Sync + 'static {
    fn variable_catalog(&self) -> Vec<CatalogEntry>;
    fn all_variables(&self) -> Vec<VariableSnapshot>;
    fn runtime_status(&self) -> String;
    fn cycle_stats(&self) -> Option<CycleStats>;
    async fn force_variable(&self, name: &str, value: &str) -> Result<String, String>;
    async fn unforce_variable(&self, name: &str) -> Result<(), String>;
}
```

The host application (st-target-agent) implements this trait by wrapping its `RuntimeManager`.

### Server Entry Point

```rust
let handle = st_opcua_server::run_opcua_server(config, provider).await?;
// Server is running — cancel via handle.cancel()
```

## Functional Description

### Architecture

```
Engine Thread              OPC-UA Server (tokio tasks)
────────────               ────────────────────────────
scan_cycle()               Value Sync Task (100ms poll)
  → snapshot ───────────── → parse string → Variant
    (Arc<RwLock>)          → update node DataValues
                           → subscription notifications

  ← force_variable ─────── Write Handler
    (command channel)       ← OPC-UA Write service
```

### Type Mapping

All IEC 61131-3 types map to OPC-UA equivalents: `BOOL` → Boolean, `INT` → Int16, `DINT` → Int32, `REAL` → Float, `LREAL` → Double, `STRING` → String, etc.

### Certificates

A self-signed application certificate is auto-generated on first startup and stored in the PKI directory. OPC-UA requires certificates even with `SecurityPolicy: None`.

### Catalog Rebuild

When the PLC program starts, stops, or undergoes online change, the address space is automatically rebuilt — old nodes removed, new nodes created from the updated variable catalog.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `async-opcua-server` | OPC-UA server implementation (pure Rust) |
| `async-opcua-types` | OPC-UA data types |
| `async-opcua-crypto` | Certificate handling (RustCrypto, no OpenSSL) |
| `async-opcua-nodes` | Node management |
| `tokio` | Async runtime |
| `tracing` | Structured logging |
| `async-trait` | Async trait support |

## Production Deployment

The OPC-UA server is embedded in the `st-runtime` binary — it is NOT a separate service. It starts automatically with the agent and shares the process with the HTTP API, WebSocket monitor, and DAP proxy.

Feature-gated via the `opcua` cargo feature (default: enabled). Build without it for smaller binaries:

```bash
cargo build --release --no-default-features -p st-target-agent
```

## Tests

- 45 unit tests (type mapping, address space construction, config parsing)
- 3 integration tests (mock provider: server startup, address space population, write callbacks)
- 3 E2E echo tests (INT, BOOL, REAL round-trip through real PLC engine)
