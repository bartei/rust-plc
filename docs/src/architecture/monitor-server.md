# Monitor Server

The monitor server (`st-monitor` crate) provides a WebSocket-based interface
for live variable observation, forced variable control, and online change
triggering. It runs alongside the scan-cycle engine and communicates using a
JSON-RPC protocol.

## Architecture

```
  ┌──────────────┐        ┌───────────────┐        ┌──────────────┐
  │ VSCode       │  WS    │ st-monitor    │        │ st-runtime   │
  │ MonitorPanel │◄──────►│ WebSocket     │◄──────►│ Engine       │
  │ (webview)    │        │ Server        │        │ (scan loop)  │
  └──────────────┘        └───────────────┘        └──────────────┘
                                │
                          MonitorHandle
                          (thread-safe)
```

The `MonitorHandle` is the bridge between the WebSocket server and the engine.
It is designed to be non-blocking: the engine publishes variable state into the
handle after each scan cycle, and the server reads it when clients request
updates. Force commands flow in the reverse direction.

## Protocol Reference

All messages use JSON over WebSocket. Requests are tagged by `method` name
with optional `params`. Responses are tagged by `type`.

### Request Format

Requests use serde's tag-content encoding:

```json
{
  "method": "<method_name>",
  "params": { ... }
}
```

### Response Types

The server sends back one of four message types:

| Type | Description |
|------|-------------|
| `response` | Success/failure response to a request |
| `variableUpdate` | Pushed variable value update for subscribers |
| `cycleInfo` | Scan cycle statistics |
| `error` | Error message |

**Response format:**
```json
{
  "type": "response",
  "id": null,
  "success": true,
  "data": { ... }
}
```

**Error format:**
```json
{
  "type": "error",
  "message": "description of the error"
}
```

## Request Types

The monitor server supports 8 request types:

---

### 1. `subscribe`

Subscribe to live variable updates. After subscribing, the server pushes
`variableUpdate` messages after each scan cycle (or at the specified interval).

**Request:**
```json
{
  "method": "subscribe",
  "params": {
    "variables": ["Main.counter", "Main.limit"],
    "interval_ms": 0
  }
}
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `variables` | `string[]` | Variable names to subscribe to |
| `interval_ms` | `u64` | Update interval in milliseconds (0 = every cycle) |

After subscribing, the server sends push notifications:
```json
{
  "type": "variableUpdate",
  "cycle": 1042,
  "variables": [
    { "name": "Main.counter", "value": "37", "type": "INT" },
    { "name": "Main.limit", "value": "50", "type": "INT" }
  ]
}
```

---

### 2. `unsubscribe`

Stop receiving updates for specific variables.

**Request:**
```json
{
  "method": "unsubscribe",
  "params": {
    "variables": ["Main.counter"]
  }
}
```

---

### 3. `read`

Read current values of specific variables (polling mode).

**Request:**
```json
{
  "method": "read",
  "params": {
    "variables": ["Main.counter", "Main.limit"]
  }
}
```

**Response:**
```json
{
  "type": "response",
  "success": true,
  "data": {
    "variables": [
      { "name": "Main.counter", "value": "37", "type": "INT" },
      { "name": "Main.limit", "value": "50", "type": "INT" }
    ]
  }
}
```

---

### 4. `write`

Write a value to a variable.

**Request:**
```json
{
  "method": "write",
  "params": {
    "variable": "Main.counter",
    "value": 100
  }
}
```

---

### 5. `force`

Override a variable's value. The forced value is written at the start of each
scan cycle, overriding whatever the program logic computes.

**Request:**
```json
{
  "method": "force",
  "params": {
    "variable": "Main.counter",
    "value": 100
  }
}
```

---

### 6. `unforce`

Remove the force override from a variable, returning it to normal program
control.

**Request:**
```json
{
  "method": "unforce",
  "params": {
    "variable": "Main.counter"
  }
}
```

---

### 7. `getCycleInfo`

Get scan cycle statistics. This method takes no parameters.

**Request:**
```json
{
  "method": "getCycleInfo"
}
```

**Response:**
```json
{
  "type": "cycleInfo",
  "cycle_count": 1042,
  "last_cycle_us": 150,
  "min_cycle_us": 120,
  "max_cycle_us": 450,
  "avg_cycle_us": 165
}
```

---

### 8. `onlineChange`

Push new source code to the running engine for hot-reload. The server
performs the full pipeline: parse, analyze, compile, compatibility analysis,
variable migration, and atomic swap.

**Request:**
```json
{
  "method": "onlineChange",
  "params": {
    "source": "PROGRAM Main\nVAR\n  counter : INT := 0;\nEND_VAR\n  counter := counter + 2;\nEND_PROGRAM"
  }
}
```

**Response (success):**
```json
{
  "type": "response",
  "success": true,
  "data": {
    "status": "applied"
  }
}
```

**Response (incompatible):**
```json
{
  "type": "error",
  "message": "incompatible change: variable 'counter' type changed from INT to DINT"
}
```

See [Online Change](./online-change.md) for details on compatibility rules.

## MonitorHandle API

The `MonitorHandle` is the Rust API used internally by the engine to communicate
with the monitor server. It is `Send + Sync` and designed for zero-copy
operation where possible.

```rust
pub struct MonitorHandle { /* ... */ }

impl MonitorHandle {
    /// Publish the current variable state after a scan cycle completes.
    /// Called by the engine at the end of each cycle.
    pub fn publish_state(&self, cycle: u64, variables: &VariableSnapshot);

    /// Check for pending force commands from connected clients.
    /// Called by the engine at the start of each cycle.
    pub fn poll_forces(&self) -> Vec<ForceCommand>;

    /// Check for a pending online change request.
    /// Returns the new module if one is queued.
    pub fn poll_online_change(&self) -> Option<Module>;

    /// Report the result of an online change back to the requesting client.
    pub fn report_change_result(&self, result: Result<MigrationReport, String>);
}
```

### Integration with the Engine

The engine integrates with the monitor handle in its scan loop:

```
  loop {
      // 1. Apply any forced variables
      for cmd in handle.poll_forces() {
          vm.force_variable(cmd.name, cmd.value);
      }

      // 2. Check for online change
      if let Some(new_module) = handle.poll_online_change() {
          let result = apply_online_change(&mut vm, new_module);
          handle.report_change_result(result);
      }

      // 3. Execute one scan cycle
      vm.run_cycle();

      // 4. Publish state to subscribers
      handle.publish_state(cycle_count, &vm.snapshot());

      cycle_count += 1;
  }
```

## VSCode MonitorPanel

The VSCode extension includes a `MonitorPanel` webview that connects to the
monitor server. Open it via:

**Command Palette (Ctrl+Shift+P) -> "ST: Open PLC Monitor"**

The panel provides:

- A variable table showing live values, types, and forced status
- Right-click context menu to force/unforce variables
- Visual indicators for forced variables
- Cycle counter showing the current scan cycle number

The panel automatically connects to the monitor server when the engine is
running and reconnects if the connection is lost.
