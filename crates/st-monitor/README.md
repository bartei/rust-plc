# st-monitor

WebSocket-based PLC monitor server for live variable observation.

## Purpose

Provides a real-time WebSocket interface for monitoring PLC variable values, cycle statistics, and forced variable state. Used by the VS Code PLC Monitor panel, the target agent's WebSocket API, and any custom monitoring clients.

## Public API

```rust
use st_monitor::{MonitorHandle, run_monitor_server};

// Create a handle (shared between engine and server)
let handle = MonitorHandle::new();

// Start the WebSocket server
let addr = run_monitor_server("127.0.0.1:0", handle.clone()).await?;

// Engine thread updates the handle after each scan cycle
handle.set_catalog(catalog);
handle.update_variables(variables, cycle_info);
```

### Key Types

- `MonitorHandle` — Thread-safe bridge between the engine and WebSocket server. The engine pushes updates; the server reads them.
- `MonitorState` — Current variable values, catalog, cycle info, forced variables, pending online changes
- `VariableValue` — Name + value (string) + type + forced flag
- `CatalogEntry` — Variable name + type (schema only)

### MonitorHandle Methods

| Method | Called by | Purpose |
|--------|----------|---------|
| `set_catalog(catalog)` | Engine (once at start) | Set variable names + types |
| `update_variables(vars, cycle_info)` | Engine (every cycle) | Push latest values |
| `has_subscribers()` | Engine | Gate expensive snapshots |
| `take_forced_variables()` | Engine | Read pending force commands |
| `take_pending_online_change()` | Engine | Read pending code update |
| `subscribe()` | Server | Get cycle notification receiver |

## Protocol

All messages use JSON over WebSocket. Clients send requests, the server sends responses and push notifications.

### Client Requests

```json
{"method": "subscribe", "params": {"variables": ["Main.counter", "io_rack_DI_0"]}}
{"method": "read", "params": {"variables": ["Main.counter"]}}
{"method": "force", "params": {"variable": "Main.counter", "value": "42"}}
{"method": "unforce", "params": {"variable": "Main.counter"}}
{"method": "getCatalog"}
{"method": "getCycleInfo"}
```

### Server Push (on subscription)

```json
{"type": "variableUpdate", "variables": [...], "cycleInfo": {...}}
```

Push messages are throttled to 50ms minimum interval to avoid flooding slow clients.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-ir` | Value types |
| `tokio` | Async runtime |
| `tokio-tungstenite` | WebSocket implementation |
| `futures-util` | Stream utilities |
| `serde`, `serde_json` | JSON serialization |
| `tracing` | Logging |
