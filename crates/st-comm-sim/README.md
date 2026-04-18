# st-comm-sim

Simulated communication devices with interactive web UI.

## Purpose

Implements the `CommDevice` and `CommLink` traits with in-memory register storage. Provides an HTTP web UI for manually toggling inputs and observing outputs — ideal for development and testing without physical hardware.

The same ST code works unchanged when switching from simulated to real hardware — only the `plc-project.yaml` changes.

## How to Use

### Configuration

Add simulated devices to your `plc-project.yaml`:

```yaml
links:
  - name: sim_link
    type: simulated

devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_8di_4ai_4do_2ao
    web_ui: true
    web_port: 8080

  - name: vfd
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_vfd
    web_ui: true
    web_port: 8081
```

### Running

```bash
st-cli run .
# → Web UIs available at:
#   http://localhost:8080  (io_rack)
#   http://localhost:8081  (vfd)
```

### Web UI

Each device gets its own web page with:
- **Input panel** — Toggle switches for digital inputs, number fields for analog inputs
- **Output panel** — LED indicators for digital outputs, value displays for analog outputs
- **Real-time updates** — Values refresh via polling every 100ms
- **Dark theme** — Industrial-style UI

## Public API

```rust
use st_comm_sim::{SimulatedDevice, SimulatedLink};
use st_comm_api::DeviceProfile;

let profile = DeviceProfile::from_file("profiles/sim_vfd.yaml")?;
let device = SimulatedDevice::new("vfd", profile);

// Get shared state handle for the web UI
let state_handle = device.state_handle();

// Programmatically set an input value
device.set_input("SPEED_ACT", IoValue::Real(45.0))?;

// Read an output value
let run_cmd = device.get_output("RUN");
```

- `SimulatedDevice` — Implements `CommDevice` with in-memory storage
- `SimulatedLink` — Implements `CommLink` as a no-op (no network)
- `web::start_web_ui(name, profile, state, port)` — Start the HTTP web UI server

## Functional Description

- All register fields initialize to defaults: `BOOL` = false, numeric = 0
- `read_inputs()` returns all input-direction fields from in-memory state
- `write_outputs()` stores output-direction fields in in-memory state
- Direction enforcement: `set_input()` rejects output fields, `write_outputs()` ignores input fields
- State is shared via `Arc<Mutex<HashMap<String, IoValue>>>` — accessible by the web UI

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-comm-api` | CommDevice/CommLink traits |
| `tokio` | Async runtime for web server |
| `tokio-tungstenite` | WebSocket support |
| `tracing` | Logging |
