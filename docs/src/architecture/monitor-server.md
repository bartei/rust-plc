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

All messages use JSON over WebSocket. Requests include an `id` field for
correlation. Responses echo the `id` back.

### Request Format

```json
{
  "id": 1,
  "method": "<method_name>",
  "params": { ... }
}
```

### Response Format

```json
{
  "id": 1,
  "result": { ... }
}
```

### Error Response Format

```json
{
  "id": 1,
  "error": {
    "code": -1,
    "message": "description of the error"
  }
}
```

## Request Types

The monitor server supports 8 request types:

---

### 1. `subscribe`

Subscribe to live variable updates. After subscribing, the server pushes
variable snapshots after each scan cycle.

**Request:**
```json
{
  "id": 1,
  "method": "subscribe",
  "params": {}
}
```

**Response:**
```json
{
  "id": 1,
  "result": { "status": "subscribed" }
}
```

After subscribing, the server sends push notifications:
```json
{
  "method": "variables",
  "params": {
    "cycle": 1042,
    "values": {
      "Main.counter": { "type": "INT", "value": 37 },
      "Main.limit": { "type": "INT", "value": 50 },
      "Main.active": { "type": "BOOL", "value": false }
    }
  }
}
```

---

### 2. `unsubscribe`

Stop receiving live variable updates.

**Request:**
```json
{
  "id": 2,
  "method": "unsubscribe",
  "params": {}
}
```

**Response:**
```json
{
  "id": 2,
  "result": { "status": "unsubscribed" }
}
```

---

### 3. `read_variables`

Read all variable values once (polling mode).

**Request:**
```json
{
  "id": 3,
  "method": "read_variables",
  "params": {}
}
```

**Response:**
```json
{
  "id": 3,
  "result": {
    "cycle": 1042,
    "values": {
      "Main.counter": { "type": "INT", "value": 37 },
      "Main.limit": { "type": "INT", "value": 50 },
      "Main.active": { "type": "BOOL", "value": false }
    }
  }
}
```

---

### 4. `force_variable`

Override a variable's value. The forced value is written at the start of each
scan cycle, overriding whatever the program logic computes.

**Request:**
```json
{
  "id": 4,
  "method": "force_variable",
  "params": {
    "name": "Main.counter",
    "value": 100
  }
}
```

**Response:**
```json
{
  "id": 4,
  "result": { "status": "forced", "name": "Main.counter" }
}
```

---

### 5. `unforce_variable`

Remove the force override from a variable, returning it to normal program
control.

**Request:**
```json
{
  "id": 5,
  "method": "unforce_variable",
  "params": {
    "name": "Main.counter"
  }
}
```

**Response:**
```json
{
  "id": 5,
  "result": { "status": "unforced", "name": "Main.counter" }
}
```

---

### 6. `list_forced`

List all currently forced variables and their forced values.

**Request:**
```json
{
  "id": 6,
  "method": "list_forced",
  "params": {}
}
```

**Response:**
```json
{
  "id": 6,
  "result": {
    "forced": [
      { "name": "Main.counter", "value": 100 },
      { "name": "Main.active", "value": true }
    ]
  }
}
```

---

### 7. `online_change`

Push a new compiled module to the running engine for hot-reload. The server
performs compatibility analysis, variable migration, and atomic swap.

**Request:**
```json
{
  "id": 7,
  "method": "online_change",
  "params": {
    "source": "PROGRAM Main\nVAR\n  counter : INT := 0;\nEND_VAR\n  counter := counter + 2;\nEND_PROGRAM"
  }
}
```

**Response (success):**
```json
{
  "id": 7,
  "result": {
    "status": "applied",
    "migrated": ["Main.counter"],
    "defaulted": [],
    "dropped": []
  }
}
```

**Response (incompatible):**
```json
{
  "id": 7,
  "error": {
    "code": -2,
    "message": "incompatible change: variable 'counter' type changed from INT to DINT"
  }
}
```

See [Online Change](./online-change.md) for details on compatibility rules.

---

### 8. `status`

Query the engine status: whether it is running, paused, or stopped, and the
current scan cycle count.

**Request:**
```json
{
  "id": 8,
  "method": "status",
  "params": {}
}
```

**Response:**
```json
{
  "id": 8,
  "result": {
    "state": "running",
    "cycle": 1042,
    "program": "Main",
    "forced_count": 2
  }
}
```

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
