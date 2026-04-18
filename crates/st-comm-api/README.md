# st-comm-api

Communication framework API for PLC I/O — traits, device profiles, and code generation.

## Purpose

Defines the extensible communication architecture for PLC field I/O. Every communication protocol (Modbus, PROFINET, OPC-UA client, etc.) is implemented as a separate crate that depends on this API crate. Device profiles (YAML files) define the mapping between physical registers and ST struct fields.

This crate contains no I/O implementations — only traits, types, and the codegen system.

## Public API

### Traits

```rust
use st_comm_api::{CommLink, CommDevice};

/// Physical transport layer (TCP, serial, simulated)
pub trait CommLink: Send + Sync {
    fn name(&self) -> &str;
    fn link_type(&self) -> &str;
    fn open(&mut self) -> Result<(), CommError>;
    fn close(&mut self) -> Result<(), CommError>;
    fn is_open(&self) -> bool;
    fn send(&mut self, data: &[u8]) -> Result<(), CommError>;
    fn receive(&mut self, buffer: &mut [u8], timeout_ms: u32) -> Result<usize, CommError>;
    fn diagnostics(&self) -> LinkDiagnostics;
}

/// Application protocol device (Modbus slave, simulated I/O, etc.)
pub trait CommDevice: Send + Sync {
    fn name(&self) -> &str;
    fn protocol(&self) -> &str;
    fn configure(&mut self, config: &serde_yaml::Value) -> Result<(), CommError>;
    fn bind_link(&mut self, link: Arc<Mutex<dyn CommLink>>) -> Result<(), CommError>;
    fn device_profile(&self) -> &DeviceProfile;
    fn read_inputs(&mut self) -> Result<IoValues, CommError>;
    fn write_outputs(&mut self, outputs: &IoValues) -> Result<(), CommError>;
    fn acyclic_request(&mut self, request: AcyclicRequest) -> Result<AcyclicResponse, CommError>;
    fn is_connected(&self) -> bool;
    fn diagnostics(&self) -> DeviceDiagnostics;
}
```

### Device Profiles

```rust
use st_comm_api::DeviceProfile;

let profile = DeviceProfile::from_file(Path::new("profiles/sim_vfd.yaml"))?;
for field in &profile.fields {
    println!("{}: {} ({:?})", field.name, field.data_type.st_type_name(), field.direction);
}
```

- `DeviceProfile` — Struct type name, vendor, fields with register mappings
- `ProfileField` — Name, `FieldDataType`, `FieldDirection` (Input/Output/Inout), `RegisterMapping`
- `RegisterMapping` — Address, kind (Coil/DiscreteInput/HoldingRegister/InputRegister/Virtual), bit position, scale/offset

### Code Generation

```rust
use st_comm_api::{write_io_map_file, CommConfig};

// Parse device config from plc-project.yaml
let config = CommConfig::from_project_yaml(&yaml_text)?;

// Generate _io_map.st with device globals
let path = write_io_map_file(project_root, &profiles, &config.devices)?;
```

Generated ST code creates flat globals named `{device}_{field}`:
```
VAR_GLOBAL
    io_rack_DI_0 : BOOL;    (* in *)
    io_rack_AI_0 : INT;     (* in, raw *)
    io_rack_DO_0 : BOOL;    (* out *)
END_VAR
```

### Configuration Types

- `CommConfig` — Parsed `links:` + `devices:` sections from `plc-project.yaml`
- `LinkConfig` — Link name, type (tcp/serial/simulated), host, port, baud, parity
- `DeviceConfig` — Device name, link, protocol, unit_id, mode, cycle_time, profile
- `EngineProjectConfig` — Engine-level settings from YAML (cycle_time, retain)

## Profile YAML Format

```yaml
name: SimVfd
vendor: Simulated
protocol: simulated
description: "Simulated Variable Frequency Drive"
fields:
  - name: RUN
    type: BOOL
    direction: output
    register: { address: 0, kind: virtual }
  - name: SPEED_REF
    type: REAL
    direction: output
    register: { address: 10, kind: virtual, unit: Hz }
  - name: SPEED_ACT
    type: REAL
    direction: input
    register: { address: 110, kind: virtual, unit: Hz }
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `serde`, `serde_yaml`, `serde_json` | Profile and config parsing |
| `thiserror` | Error types |

## Bundled Profiles

Profiles shipped with the project (in `profiles/` at the workspace root):
- `sim_8di_4ai_4do_2ao` — 8 digital inputs, 4 analog inputs, 4 digital outputs, 2 analog outputs
- `sim_vfd` — Simulated variable frequency drive
