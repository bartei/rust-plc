# Communication Layer — Implementation Plan

> **Parent plan:** [implementation.md](implementation.md) — core platform (Phases 0-12), cross-cutting concerns.
> **See also:** [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets (Phase 14).

## Phase 13: Communication Extension System & Modbus Implementation

A PLC is only useful if it can talk to the physical world. This phase establishes the
**communication extension architecture** — a modular, plugin-based system where each
protocol (Modbus, Profinet, EtherCAT, etc.) is an independent, versioned extension —
and delivers the first two implementations: Modbus TCP and Modbus RTU/ASCII.

### Competitive Analysis — What We Take From the Best

Based on analysis of CODESYS 3.5, Siemens TIA Portal, Beckhoff TwinCAT 3, Rockwell
Studio 5000, and Phoenix Contact PLCnext:

| Concept | Inspired By | Our Approach |
|---------|------------|--------------|
| **Auto-generated structured tags** | Studio 5000 | Device profiles → ST struct types + named global instances |
| **Decoupled I/O and PLC namespaces** | TwinCAT linked variables | Struct instances are the link — profile defines I/O shape, YAML name binds it |
| **Universal device description import** | CODESYS | Profile YAML can be hand-written or generated from GSD/ESI/EDS import tools |
| **Multi-rate I/O with task binding** | TIA Portal process image partitions | Each device declares its `cycle_time`; comm manager groups by rate |
| **Shared data space** | PLCnext GDS | Global struct instances ARE the shared data space — VM, comm manager, monitor all access them |
| **Text-based, git-friendly config** | *None (we're first)* | YAML for project config + device profiles; diffs, code review, CI/CD all work |
| **Layer separation** | OSI / CODESYS | Links (physical) → Devices (protocol) → Profiles (schema) → Globals (binding) |

**What we do that nobody else does:**
- **YAML-first configuration** — every competitor uses proprietary binary or heavyweight XML
  inside IDE project databases. Ours is human-readable, git-diffable, CI/CD-friendly.
- **Profile = struct type + register map in one file** — competitors separate device description
  from I/O mapping. We unify them: one YAML file defines both the ST data structure and the
  register-level wiring. Share a profile, get both the code interface and the hardware mapping.
- **No IDE required** — configure hardware with a text editor. Every competitor requires their
  proprietary IDE for hardware configuration.
- **Cross-protocol profiles** — a device profile defines field names and types independent of
  transport. The same ABB ACS580 profile works whether you're talking Modbus TCP, Modbus RTU,
  or (future) PROFINET — only the link and register mapping change.

### Design Principles

1. **OSI-layered architecture** — physical links, protocol devices, and application-level
   profiles are separate concerns in separate crates
2. **Each protocol is an independent crate** — separately versioned, tested, and maintained
3. **No framework recompilation** — extensions are loaded via trait interfaces
4. **Community extensible** — third parties can publish protocol extensions and device profiles
5. **Device profiles as struct schemas** — each profile defines an ST struct type + register map;
   each YAML device entry becomes a named global instance of that struct
6. **Cyclic + acyclic modes** — cyclic I/O every scan cycle, acyclic on-demand
7. **Multi-rate I/O** — each device can have its own cycle_time; faster devices update more often
8. **Diagnostics built in** — every link and device exposes health, error counters, and connection
   state as additional struct fields (like Studio 5000's module fault bits)

### Architecture

Follows OSI-inspired layer separation: **links** (Layer 1-2: physical transport) are
separate from **devices** (Layer 7: application protocol). A single link can carry
multiple devices (e.g., multiple Modbus slaves on one RS-485 bus).

```
┌─────────────────────────────────────────────────────────────┐
│                     ST Program (Layer 7)                      │
│   IF rack_left.DI_0 THEN rack_right.DO_3 := TRUE; END_IF;   │
│   pump_vfd.SPEED_REF := 45.0;                               │
│   fan_vfd.RUN := TRUE;  (* same bus, different address *)    │
└──────────────┬───────────────────────────┬──────────────────┘
               │ Read Inputs               │ Write Outputs
               │ (struct fields ← regs)    │ (struct fields → regs)
┌──────────────▼───────────────────────────▼──────────────────┐
│         Communication Manager (orchestrator)                 │
│                                                              │
│  Device Layer (protocol + profiles)                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐            │
│  │ rack_left   │ │ pump_vfd    │ │ fan_vfd     │            │
│  │ Modbus TCP  │ │ Modbus RTU  │ │ Modbus RTU  │            │
│  │ unit_id=1   │ │ unit_id=3   │ │ unit_id=4   │            │
│  │ wago_750    │ │ abb_acs580  │ │ abb_acs580  │            │
│  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘            │
│         │               │               │                    │
│  Link Layer (physical transport)        │                    │
│  ┌──────▼──────┐ ┌──────▼───────────────▼──────┐            │
│  │ eth_rack_l  │ │ rs485_bus_1                  │            │
│  │ TCP         │ │ /dev/ttyUSB0, 19200 8E1      │            │
│  │ 192.168.1.  │ │ (shared by pump + fan VFDs)  │            │
│  │ 100:502     │ │                              │            │
│  └──────┬──────┘ └──────────────┬───────────────┘            │
└─────────┼───────────────────────┼────────────────────────────┘
          │                       │
    TCP/IP network          RS-485 bus
          │                       │
    ┌─────▼─────┐     ┌─────▼────┐ ┌─────▼────┐
    │  WAGO     │     │  ABB     │ │  ABB     │
    │  750-352  │     │  ACS580  │ │  ACS580  │
    │  I/O rack │     │  pump    │ │  fan     │
    └───────────┘     └──────────┘ └──────────┘
```

### Trait Architecture (Layered)

The trait design mirrors the link/device separation. Link traits manage the physical
transport. Device traits manage the protocol and register mapping. The Communication
Manager composes them.

```rust
/// Link layer: manages a physical transport channel.
/// One link can serve multiple devices (e.g., RS-485 bus with multiple slaves).
pub trait CommLink: Send + Sync {
    fn name(&self) -> &str;
    fn link_type(&self) -> &str;  // "tcp", "serial", "udp", etc.

    /// Open the physical channel with the configured settings.
    fn open(&mut self) -> Result<(), CommError>;
    fn close(&mut self) -> Result<(), CommError>;
    fn is_open(&self) -> bool;

    /// Raw data exchange (used by device layer).
    fn send(&mut self, data: &[u8]) -> Result<(), CommError>;
    fn receive(&mut self, buffer: &mut [u8], timeout_ms: u32) -> Result<usize, CommError>;

    fn diagnostics(&self) -> LinkDiagnostics;
}

/// Device layer: protocol-specific communication with a single addressable unit.
/// Reads/writes device registers and maps them to/from struct fields.
pub trait CommDevice: Send + Sync {
    fn name(&self) -> &str;
    fn protocol(&self) -> &str;  // "modbus-tcp", "modbus-rtu", "profinet", etc.

    /// Configure with the device section from plc-project.yaml.
    fn configure(&mut self, config: &serde_yaml::Value) -> Result<(), String>;

    /// Bind to a link (the device uses this link for all I/O).
    fn bind_link(&mut self, link: Arc<Mutex<dyn CommLink>>) -> Result<(), CommError>;

    /// Return the device profile (struct schema + register map).
    fn device_profile(&self) -> &DeviceProfile;

    /// Cyclic I/O: read input registers → struct field values.
    fn read_inputs(&mut self) -> Result<HashMap<String, Value>, CommError>;

    /// Cyclic I/O: struct field values → write output registers.
    fn write_outputs(&mut self, outputs: &HashMap<String, Value>) -> Result<(), CommError>;

    /// Acyclic request: on-demand read/write.
    fn acyclic_request(&mut self, request: AcyclicRequest) -> Result<AcyclicResponse, CommError>;

    fn is_connected(&self) -> bool;
    fn diagnostics(&self) -> DeviceDiagnostics;
}
```

The Communication Manager creates links from the `links:` section and devices from the
`devices:` section, binding each device to its declared link. Multiple devices sharing
a link use coordinated access (mutex/queue) to avoid bus collisions.

### Configuration in plc-project.yaml

The YAML is the **single source of truth** between hardware configuration and software
symbol mapping. Each communication entry defines a **named instance** of a device profile.
The `name` field becomes the global variable name in ST — giving a clear, unambiguous
correlation between physical hardware and code.

The YAML separates **links** (physical/transport layer) from **devices** (application
layer), following OSI layering principles. A link defines the shared transport — a
serial bus or a TCP endpoint. Devices are the addressable units on that link.

```yaml
name: BottleFillingLine
target: host

# ─── Links: physical/transport layer ─────────────────────────
# Each link is a communication channel with its own physical settings.
# Multiple devices can share a single link (same bus/connection).
links:
  # Ethernet link — one TCP endpoint per remote host
  - name: eth_rack_left
    type: tcp
    host: 192.168.1.100
    port: 502
    timeout: 500ms

  - name: eth_rack_right
    type: tcp
    host: 192.168.1.101
    port: 502
    timeout: 500ms

  # RS-485 serial bus — one port, shared by all slaves on the wire
  - name: rs485_bus_1
    type: serial
    port: /dev/ttyUSB0
    baud: 19200
    parity: even
    data_bits: 8
    stop_bits: 1
    timeout: 200ms

  # Second serial bus (different physical settings = different wire)
  - name: rs485_bus_2
    type: serial
    port: /dev/ttyUSB1
    baud: 9600
    parity: none
    data_bits: 8
    stop_bits: 2
    timeout: 500ms

  # TCP link for acyclic parameter access
  - name: eth_neighbor
    type: tcp
    host: 192.168.1.200
    port: 502

# ─── Devices: application/protocol layer ─────────────────────
# Each device is an addressable unit on a link. The `name` becomes
# the global struct instance name in ST code.
devices:
  # Two identical I/O racks on separate TCP links
  - name: rack_left              # ← VAR_GLOBAL rack_left : Wago750352;
    link: eth_rack_left
    protocol: modbus-tcp
    unit_id: 1
    mode: cyclic
    cycle_time: 10ms
    device_profile: wago_750_352

  - name: rack_right             # ← VAR_GLOBAL rack_right : Wago750352;
    link: eth_rack_right
    protocol: modbus-tcp
    unit_id: 1
    mode: cyclic
    cycle_time: 10ms
    device_profile: wago_750_352

  # Two VFDs on the SAME RS-485 bus — different slave addresses
  - name: pump_vfd               # ← VAR_GLOBAL pump_vfd : AbbAcs580;
    link: rs485_bus_1
    protocol: modbus-rtu
    unit_id: 3
    mode: cyclic
    device_profile: abb_acs580

  - name: fan_vfd                # ← VAR_GLOBAL fan_vfd : AbbAcs580;
    link: rs485_bus_1             # same bus! different address
    protocol: modbus-rtu
    unit_id: 4
    mode: cyclic
    device_profile: abb_acs580

  # Temperature sensor on a different serial bus (9600 baud)
  - name: temp_sensor
    link: rs485_bus_2
    protocol: modbus-rtu
    unit_id: 1
    mode: cyclic
    cycle_time: 100ms
    device_profile: generic_temp_rtd

  # Acyclic-only connection for on-demand parameter reads
  - name: plc_neighbor
    link: eth_neighbor
    protocol: modbus-tcp
    unit_id: 1
    mode: acyclic
```

This auto-generates struct types from device profiles and named global instances.
Every device struct automatically includes a `_diag` sub-struct with connection
health fields (inspired by Studio 5000's auto-generated module fault bits and
TIA Portal's diagnostic integration):

```st
(* Auto-generated from device profiles — DO NOT EDIT *)

(* Diagnostics sub-struct — added to every device automatically *)
TYPE CommDiag : STRUCT
    connected    : BOOL;      (* TRUE when device is responding *)
    error        : BOOL;      (* TRUE on communication error *)
    error_count  : DINT;      (* cumulative error count *)
    last_update  : TIME;      (* timestamp of last successful I/O *)
    response_ms  : INT;       (* last response time in ms *)
END_STRUCT;

(* Struct type generated from profile: wago_750_352 *)
TYPE Wago750352 : STRUCT
    (* Process I/O fields — from device profile *)
    DI_0 : BOOL;  DI_1 : BOOL;  DI_2 : BOOL;  DI_3 : BOOL;
    DI_4 : BOOL;  DI_5 : BOOL;  DI_6 : BOOL;  DI_7 : BOOL;
    AI_0 : INT;   AI_1 : INT;   AI_2 : INT;   AI_3 : INT;
    DO_0 : BOOL;  DO_1 : BOOL;  DO_2 : BOOL;  DO_3 : BOOL;
    AO_0 : INT;   AO_1 : INT;
    (* Connection diagnostics — auto-generated *)
    _diag : CommDiag;
END_STRUCT;

(* Struct type generated from profile: abb_acs580 *)
TYPE AbbAcs580 : STRUCT
    RUN        : BOOL;     (* control word bit 0, output *)
    STOP       : BOOL;     (* control word bit 1, output *)
    FAULT_RST  : BOOL;     (* control word bit 7, output *)
    READY      : BOOL;     (* status word bit 0, input *)
    RUNNING    : BOOL;     (* status word bit 1, input *)
    FAULT      : BOOL;     (* status word bit 3, input *)
    SPEED_REF  : REAL;     (* holding register 1, 0.1 Hz, output *)
    SPEED_ACT  : REAL;     (* input register 2, 0.1 Hz, input *)
    CURRENT    : REAL;     (* input register 3, 0.1 A, input *)
    TORQUE     : REAL;     (* input register 4, 0.1 Nm, input *)
    POWER      : REAL;     (* input register 5, 0.1 kW, input *)
    _diag      : CommDiag;
END_STRUCT;
END_TYPE

(* Global instances — names from plc-project.yaml *)
VAR_GLOBAL
    rack_left  : Wago750352;   (* eth_rack_left, unit 1 *)
    rack_right : Wago750352;   (* eth_rack_right, unit 1 *)
    pump_vfd   : AbbAcs580;    (* rs485_bus_1, unit 3 *)
    fan_vfd    : AbbAcs580;    (* rs485_bus_1, unit 4 *)
END_VAR
```

User code is clear, portable, and hardware-agnostic. Diagnostics are
available without any extra setup — just read the `_diag` fields:

```st
PROGRAM Main
VAR
    motor_on : BOOL;
END_VAR
    (* Unambiguous: which rack, which channel *)
    IF rack_left.DI_0 AND NOT rack_left.DI_7 THEN
        rack_right.DO_3 := TRUE;
    END_IF;

    (* VFD control — readable field names from the profile *)
    pump_vfd.RUN := motor_on;
    pump_vfd.SPEED_REF := 45.0;

    IF pump_vfd.FAULT THEN
        pump_vfd.FAULT_RST := TRUE;
    END_IF;

    (* Built-in diagnostics — no setup required *)
    IF NOT rack_left._diag.connected THEN
        (* rack_left is offline — safe state *)
        rack_right.DO_0 := FALSE;
        rack_right.DO_1 := FALSE;
    END_IF;

    IF pump_vfd._diag.error_count > 10 THEN
        (* too many comm errors — stop the VFD *)
        pump_vfd.RUN := FALSE;
    END_IF;

    (* Swap hardware? Change YAML, code stays the same. *)
END_PROGRAM
```

**Key benefits of the struct-based approach:**
- **No name collisions** — two identical cards don't fight over `DI_0`
- **Self-documenting** — `rack_left.DI_3` is unambiguous in code
- **Portability** — change `device_profile` or connection params in YAML, code unchanged
- **Reusable profiles** — define `wago_750_352.yaml` once, share across projects
- **Type safety** — the compiler knows which fields exist on each device
- **YAML as single source of truth** — hardware config and symbol mapping in one place

### Simulated Device (First Implementation)

The simulated device is the first `CommDevice` implementation — no hardware needed.
It uses in-memory register storage and exposes a web UI for manual I/O testing.
The same device profile YAML format is used for both simulated and real devices,
so switching from simulation to hardware is just a YAML config change.

```yaml
# plc-project.yaml — simulated devices for development/testing
links:
  - name: sim_link
    type: simulated          # in-memory, no network

devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_8di_4ai_4do_2ao
    web_ui: true             # expose web UI for this device
    web_port: 8080           # default port

  - name: vfd_sim
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_vfd
    web_ui: true
    web_port: 8081
```

The ST code is identical to what it would be with real hardware:

```st
PROGRAM Main
VAR
    motor_on : BOOL;
END_VAR
    (* Works with simulated device — toggle DI_0 in the web UI *)
    IF io_rack.DI_0 THEN
        io_rack.DO_0 := TRUE;
    END_IF;

    (* VFD simulation — set speed in the web UI, see output *)
    vfd_sim.RUN := motor_on;
    vfd_sim.SPEED_REF := 45.0;

    (* Later: change YAML to protocol: modbus-tcp → same code, real hardware *)
END_PROGRAM
```

The web UI (served at `localhost:8080`) provides:
- **Inputs panel**: toggle switches for digital inputs, sliders/numeric for analog inputs
- **Outputs panel**: LED indicators for digital outputs, value displays for analog outputs
- **Diagnostics**: cycle count, last update timestamp, I/O direction arrows
- **Real-time updates**: WebSocket pushes every scan cycle

### Multi-Rate I/O

Inspired by TIA Portal's process image partitions, each device can declare its own
`cycle_time`. The Communication Manager groups devices by rate and updates them
independently. Fast devices (safety I/O, motion) update every cycle; slow devices
(temperature sensors, energy meters) update less often, reducing bus load:

```yaml
devices:
  - name: safety_io
    cycle_time: 1ms         # every scan cycle
    device_profile: safety_module
    # ...
  - name: temp_sensor
    cycle_time: 500ms       # every 500th cycle at 1ms scan
    device_profile: generic_temp_rtd
    # ...
```

The comm manager tracks a per-device timer. Input fields of slow devices hold their
last-known value between updates; `_diag.last_update` lets user code detect staleness.

### Device Description Import (Future)

Inspired by CODESYS's universal import capability. A CLI tool converts standard device
description files into our YAML profile format:

```bash
st-cli profile import --format gsdml    device.xml     # PROFINET
st-cli profile import --format esi      device.xml     # EtherCAT
st-cli profile import --format eds      device.eds     # CANopen / EtherNet/IP
```

This generates a `.yaml` profile with the struct fields and register mappings extracted
from the standard file. Users can then edit the generated YAML to customize field names,
add scaling, or remove unused channels.

### Device Profile System

A device profile is a reusable YAML file that defines **both** the struct schema (fields
visible in ST code) **and** the register map (how the communication runtime reads/writes
the physical device). Profiles can be shared between projects and published as a community
library.

Each profile defines:
1. **Struct type name** — becomes the TYPE name in generated ST code
2. **Fields** — each field has a name, ST data type, direction, and register mapping
3. **Register mapping** — Modbus register address, type, bit position, scaling

```yaml
# profiles/abb_acs580.yaml
name: AbbAcs580
vendor: ABB
protocol: modbus-rtu
description: "Standard Modbus register map for ABB ACS580 series VFDs"

fields:
  # Control outputs (ST writes → Modbus writes)
  - name: RUN
    type: BOOL
    direction: output
    register: { address: 0, bit: 0, kind: coil }

  - name: STOP
    type: BOOL
    direction: output
    register: { address: 0, bit: 1, kind: coil }

  - name: FAULT_RST
    type: BOOL
    direction: output
    register: { address: 0, bit: 7, kind: coil }

  # Status inputs (Modbus reads → ST reads)
  - name: READY
    type: BOOL
    direction: input
    register: { address: 0, bit: 0, kind: discrete_input }

  - name: RUNNING
    type: BOOL
    direction: input
    register: { address: 0, bit: 1, kind: discrete_input }

  - name: FAULT
    type: BOOL
    direction: input
    register: { address: 0, bit: 3, kind: discrete_input }

  # Analog I/O (scaled values)
  - name: SPEED_REF
    type: REAL
    direction: output
    register: { address: 1, kind: holding_register, scale: 0.1, unit: Hz }

  - name: SPEED_ACT
    type: REAL
    direction: input
    register: { address: 2, kind: input_register, scale: 0.1, unit: Hz }

  - name: CURRENT
    type: REAL
    direction: input
    register: { address: 3, kind: input_register, scale: 0.1, unit: A }

  - name: TORQUE
    type: REAL
    direction: input
    register: { address: 4, kind: input_register, scale: 0.1, unit: Nm }

  - name: POWER
    type: REAL
    direction: input
    register: { address: 5, kind: input_register, scale: 0.1, unit: kW }
```

A generic I/O module profile shows the pattern for digital/analog boards:

```yaml
# profiles/wago_750_352.yaml
name: Wago750352
vendor: WAGO
protocol: modbus-tcp
description: "WAGO 750-352 fieldbus coupler with 8 DI, 4 AI, 4 DO, 2 AO"

fields:
  - { name: DI_0, type: BOOL, direction: input,  register: { address: 0, bit: 0, kind: coil } }
  - { name: DI_1, type: BOOL, direction: input,  register: { address: 0, bit: 1, kind: coil } }
  - { name: DI_2, type: BOOL, direction: input,  register: { address: 0, bit: 2, kind: coil } }
  - { name: DI_3, type: BOOL, direction: input,  register: { address: 0, bit: 3, kind: coil } }
  - { name: DI_4, type: BOOL, direction: input,  register: { address: 0, bit: 4, kind: coil } }
  - { name: DI_5, type: BOOL, direction: input,  register: { address: 0, bit: 5, kind: coil } }
  - { name: DI_6, type: BOOL, direction: input,  register: { address: 0, bit: 6, kind: coil } }
  - { name: DI_7, type: BOOL, direction: input,  register: { address: 0, bit: 7, kind: coil } }
  - { name: AI_0, type: INT,  direction: input,  register: { address: 0, kind: input_register } }
  - { name: AI_1, type: INT,  direction: input,  register: { address: 1, kind: input_register } }
  - { name: AI_2, type: INT,  direction: input,  register: { address: 2, kind: input_register } }
  - { name: AI_3, type: INT,  direction: input,  register: { address: 3, kind: input_register } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, bit: 0, kind: coil } }
  - { name: DO_1, type: BOOL, direction: output, register: { address: 0, bit: 1, kind: coil } }
  - { name: DO_2, type: BOOL, direction: output, register: { address: 0, bit: 2, kind: coil } }
  - { name: DO_3, type: BOOL, direction: output, register: { address: 0, bit: 3, kind: coil } }
  - { name: AO_0, type: INT,  direction: output, register: { address: 0, kind: holding_register } }
  - { name: AO_1, type: INT,  direction: output, register: { address: 1, kind: holding_register } }
```

### Extension Crate Structure

The crate layout mirrors the layer separation. Link implementations and device/protocol
implementations are separate. Device profiles are protocol-agnostic YAML.

```
st-comm-api/                    # Shared traits + types (lightweight, no I/O)
├── Cargo.toml
└── src/
    ├── lib.rs                  # CommLink + CommDevice traits
    ├── types.rs                # Value, CommError, LinkDiagnostics, etc.
    └── profile.rs              # DeviceProfile schema + YAML parser

st-comm-link-tcp/               # Link: TCP socket implementation
├── Cargo.toml                  # depends on st-comm-api
├── src/
│   └── lib.rs                  # implements CommLink for TCP
└── tests/

st-comm-link-serial/            # Link: serial port (RS-485/RS-232)
├── Cargo.toml
├── src/
│   └── lib.rs                  # implements CommLink for serial
└── tests/

st-comm-modbus/                 # Device: Modbus protocol (TCP + RTU framing)
├── Cargo.toml                  # depends on st-comm-api (NOT on link crates)
├── src/
│   ├── lib.rs                  # implements CommDevice for Modbus
│   ├── tcp_framing.rs          # MBAP header framing (for TCP links)
│   ├── rtu_framing.rs          # RTU framing + CRC-16 (for serial links)
│   ├── ascii_framing.rs        # ASCII framing + LRC (for serial links)
│   └── registers.rs            # Coil/register read/write logic
└── tests/

profiles/                       # Device profiles (shared across protocols)
├── wago_750_352.yaml           # WAGO I/O coupler
├── abb_acs580.yaml             # ABB VFD
├── siemens_g120.yaml           # Siemens VFD
├── danfoss_fc302.yaml          # Danfoss VFD
├── generic_io_16di.yaml        # Generic 16-ch digital input
├── generic_temp_rtd.yaml       # Generic RTD temperature sensor
└── README.md                   # How to create a device profile
```

**Why this structure?** A Modbus device doesn't care whether it's on TCP or serial —
the protocol framing changes, but the register map is the same. The `st-comm-modbus`
crate detects the link type and selects the appropriate framing (MBAP for TCP, RTU/ASCII
for serial). Adding a new transport (e.g., UDP, Bluetooth serial) only requires a new
link crate — all existing device crates work unchanged.

### Scan Cycle Integration

```
┌────────────────────────────────────────────────────┐
│              Engine Scan Cycle                       │
│                                                     │
│  1. comm_manager.read_inputs()                      │
│     → For each cyclic device:                       │
│       → Read Modbus registers from physical device  │
│       → Map register values → struct fields         │
│       → Write struct fields into VM globals          │
│       (e.g., rack_left.DI_0, pump_vfd.SPEED_ACT)    │
│                                                     │
│  2. vm.scan_cycle("Main")                           │
│     → Execute user's ST program                     │
│     → Program reads rack_left.DI_0, writes          │
│       pump_vfd.SPEED_REF, etc.                      │
│                                                     │
│  3. comm_manager.write_outputs()                    │
│     → For each cyclic device:                       │
│       → Read struct fields from VM globals           │
│       → Map struct fields → register values         │
│       → Write Modbus registers to physical device   │
│       (only output-direction fields are written)     │
│                                                     │
│  4. comm_manager.process_acyclic()                  │
│     → Handle queued on-demand requests              │
└────────────────────────────────────────────────────┘
```

### Implementation Plan

Implementation order: API crate → simulated device (for testing) → communication
manager → engine integration → then real protocol implementations (Modbus, etc.).

#### Phase 13a: Core API + Simulated Device (build and test the framework)

Status: **mostly complete** — core framework, simulated device, web UI, scan-cycle
integration, CLI/DAP wiring, on-disk symbol map, configurable cycle time with
jitter tracking, live PLC Monitor with watch list and force/unforce, integer
overflow wrapping, literal context typing, global variable initialization, and
a working playground are all in place on `feature/phase13-comm-framework`.
Outstanding items are advanced features (multi-rate scheduling, register scaling,
diagnostics surface, Siemens-style watch tables) that aren't blocking the first
end-to-end demo.

- [x] **`st-comm-api` crate** (shared traits + types):
  - [x] `CommLink` trait (open, close, send, receive, diagnostics)
  - [x] `CommDevice` trait (configure, bind_link, read_inputs, write_outputs, acyclic)
  - [x] `DeviceProfile` struct (name, vendor, fields with register mappings)
  - [x] `ProfileField` struct (name, ST type, direction, register address/kind/bit/scale)
  - [x] `CommError`, `LinkDiagnostics`, `DeviceDiagnostics` types
  - [x] `AcyclicRequest`/`AcyclicResponse` types
  - [x] Device profile YAML parser (profile → struct schema + register map)
  - [x] Profile-to-ST code generator (emits flat `{device}_{field}` globals with
        a column-aligned mapping table in comments — Codesys/TwinCAT-style)
  - [x] Project YAML parser (`links:` + `devices:` sections)
  - [x] `write_io_map_file()`: writes `{project_root}/_io_map.st` only if changed
- [x] **`st-comm-sim` crate** (simulated device — first CommDevice implementation):
  - [x] Implements `CommDevice` trait with in-memory register storage
  - [x] Simulated link (no network — direct in-memory reads/writes)
  - [x] Web UI server (HTTP + JSON polling, one port per device starting at 8080):
    - [x] Toggle digital inputs (DI_0..DI_n) with switches
    - [x] Set analog inputs (AI_0..AI_n) with numeric fields
    - [x] Display digital output states (DO_0..DO_n) as LED indicators
    - [x] Display analog output values (AO_0..AO_n)
    - [ ] Show device diagnostics (connected, cycle count, last update)
    - ~~Real-time updates via WebSocket~~ → replaced with HTTP polling at 200ms;
          simpler, no extra deps, plenty fast for desktop simulation
  - [x] Loads standard device profile YAML (same format as real hardware)
  - [x] Multiple simulated devices per project (each gets its own web panel)
  - [x] Unit tests: register read/write, profile loading, I/O direction enforcement
  - [x] Integration test: full scan cycle with simulated device + ST program
        (`playground/sim_project` exercises this end-to-end)
- [x] **Communication Manager** (in `st-runtime/src/comm_manager.rs`):
  - [x] Parse `links:` and `devices:` sections from plc-project.yaml
        (handled in CLI/DAP `comm_setup` modules)
  - [x] Create device instances, register them with the engine's CommManager
  - [ ] Coordinate bus access for shared links (mutex/queue for serial buses) —
        deferred to Phase 13b when real links exist
  - [x] Integrate into scan cycle: `read_inputs` → execute → `write_outputs`
  - [x] Map device profile fields ↔ VM globals via `{device}_{field}` slots
  - [x] Direction-aware I/O: only read input fields, only write output fields
  - [ ] Register value scaling (raw register ↔ engineering units via `scale`)
  - [ ] Multi-rate scheduling: per-device `cycle_time` with independent timers
  - [ ] Auto-generate `CommDiag` fields per device (connected, error, etc.) —
        see "Diagnostics exposure" subsection below for the agreed design
  - [ ] Connection monitoring and automatic reconnection with backoff
  - [ ] Diagnostics exposed via monitor server
- [x] **Engine + CLI + DAP integration**:
  - [x] `Engine` owns a `CommManager`, calls `read_inputs` / `write_outputs` per scan
  - [x] `Engine::register_comm_device()` helper to avoid borrow-split at call sites
  - [x] `Vm::set_global_by_slot` / `get_global_by_slot` for fast slot-based I/O
  - [x] `st-cli run` loads link/device config, regenerates `_io_map.st`,
        instantiates sim devices, registers them, and starts web UIs
  - [x] `st-cli check` regenerates `_io_map.st` so the LSP sees the same globals
  - [x] `st-cli comm-gen [path]` for explicit regeneration
  - [x] DAP server does the same setup before launch — debugging the playground
        from VS Code Just Works (breakpoints, stepping, web UI all live at once)
  - [x] `read_inputs`/`write_outputs` called at every DAP scan-cycle boundary
  - [ ] `st-cli comm-status` shows link health and device connection state
  - [ ] `st-cli profile validate` checks a device profile YAML for errors
- [x] **On-disk symbol map (Codesys/TwinCAT-style mapping table)**:
  - [x] `_io_map.st` is written to project root, regenerated only when contents change
  - [x] File is gitignored (auto-generated artifact)
  - [x] Human-readable header per device (link, protocol, mode, vendor, description)
  - [x] Column-aligned mapping table in comments: GLOBAL | FIELD | DIR | TYPE | REGISTER | UNIT
  - [x] Picked up by project autodiscovery → LSP, semantic checker, compiler,
        runtime, and DAP all see the same globals from one source on disk
- [x] **Bundled device profiles** (`profiles/`):
  - [x] `sim_8di_4ai_4do_2ao` — 8 DI, 4 AI, 4 DO, 2 AO
  - [ ] `sim_16di_16do` — 16-channel digital I/O
  - [x] `sim_vfd` — simulated VFD (run, stop, speed_ref, speed_act, current,
        torque, power, fault)
- [x] **Playground example**: `playground/sim_project/` with `plc-project.yaml`
      wiring `io_rack` (Sim8DI4AI4DO2AO) on port 8080 and `pump_vfd` (SimVfd)
      on port 8081, plus a `main.st` showing digital passthrough, analog
      passthrough, and a VFD start/stop interlock
- [ ] **Documentation**: simulated device quickstart + "How to create a device profile"

#### Phase 13a.1: Diagnostics Exposure (HMI / SCADA integration)

Goal: provide a reliable, convenient, well-documented way for FUXA, Node-RED,
and similar HMI/SCADA tools to read device diagnostics. Two-layer design — one
ground truth (ST globals), one convenience layer (HTTP JSON).

**Layer 1 — diagnostics as auto-generated ST globals (ground truth)**
- [ ] At `CommManager::register_device()`, reserve six diag globals per device
      using the existing `{device}_{field}` flat-naming convention:
      - `{device}_diag_connected`    : BOOL   (responding this cycle)
      - `{device}_diag_error`        : BOOL   (last transaction failed)
      - `{device}_diag_error_count`  : UDINT  (cumulative errors)
      - `{device}_diag_cycles_ok`    : UDINT  (successful scan cycles)
      - `{device}_diag_last_resp_ms` : UINT   (last round-trip time)
      - `{device}_diag_last_update`  : UDINT  (engine cycle of last good I/O)
- [ ] After `write_outputs()` in the scan cycle, call `device.diagnostics()`
      and write the six fields onto their reserved global slots
- [ ] `_io_map.st` emits a trailing `--- DIAGNOSTICS ---` block per device so
      LSP / semantic checker / user ST code all see the diag globals
- [ ] Same treatment for links: `{link}_link_is_open`, `{link}_link_bytes_sent`,
      `{link}_link_bytes_received`, `{link}_link_errors` (deferred until real
      `CommLink` implementations exist in Phase 13b)
- [ ] Engine-level globals: `engine_cycle_count`, `engine_last_cycle_us`,
      `engine_min_cycle_us`, `engine_max_cycle_us`, `engine_avg_cycle_us`
- [ ] Unit test: globals exist after `register_device`, get updated each cycle,
      readable from ST code (e.g., `IF NOT io_rack_diag_connected THEN ...`)

**Layer 2 — HTTP `/api/diagnostics` JSON endpoint (convenience layer)**
- [ ] New `st-diag-server` (or fold into `st-monitor`) running on a SEPARATE
      port from the monitor WebSocket — declared in `plc-project.yaml`:
      ```yaml
      diagnostics:
        port: 9090
        bind: 127.0.0.1
      ```
      Separate port because HMIs and the monitor UI have different auth/CORS
      profiles down the line.
- [ ] `GET /api/diagnostics` — full snapshot:
      ```json
      {
        "schema": "1",
        "ts_ms": 1775692800123,
        "engine":  { "cycle_count": ..., "last_us": ..., "min_us": ...,
                     "max_us": ..., "avg_us": ... },
        "links":   { "<link>": { "is_open": ..., "bytes_sent": ..., ... } },
        "devices": { "<device>": { "protocol": ..., "profile": ...,
                                    "connected": ..., "error": ...,
                                    "error_count": ..., "successful_cycles": ...,
                                    "last_response_ms": ..., "last_error": ...,
                                    "last_update_cycle": ... } }
      }
      ```
- [ ] `GET /api/diagnostics/devices/{name}` — single device
- [ ] `GET /api/diagnostics/summary` — `{ healthy, device_count,
      connected_count, error_count }` for a single "system OK" lamp
- [ ] Stable `"schema": "1"` field so HMI configs survive future changes
- [ ] Read-only endpoint, no auth in v1 — bind to `127.0.0.1` by default

**Layer 3 — documentation (`docs/comm/diagnostics.md`)**
- [ ] Field-by-field reference for the six diag fields (units, semantics,
      when `connected` flips, update timing relative to scan cycle)
- [ ] ST code example: alarm + watchdog using `*_diag_connected`
- [ ] `/api/diagnostics` schema reference + versioning policy
- [ ] **Node-RED quickstart**: example flow JSON polling `/api/diagnostics`
      with an `inject` → `http request` → `json` → `switch` → notification
- [ ] **FUXA quickstart**: Web API device pointed at `/api/diagnostics` with
      tag bindings + a 4-lamp connection panel screenshot
- [ ] Cross-link from Phase 13a quickstart so users find it from day one

#### Phase 13a.2: VS Code Cycle-Time Feedback

Goal: give users live, glanceable feedback about scan cycle health while they
debug, using DAP custom events + native VS Code primitives.

**Tier 1 — fix `scanCycleInfo` and route DAP through real cycle stats**
- [x] **Bug**: `handle_cycle_info` reported `cycle_count = 0` because the DAP
      ran its own scan loop bypassing `Engine::run_one_cycle()`
- [x] DAP session now owns its own `CycleStats` and times each cycle in
      `step_one_dap_iteration` (the refactored loop body)
- [x] `handle_cycle_info` reports real `cycle_count`, `last_us`, `min_us`,
      `max_us`, `avg_us`, `instructions/cycle`, watchdog margin

**Tier 2 — live status bar via `plc/cycleStats` custom DAP event**
- [x] DAP server emits cycle stats every N cycles (default 20). The dap crate
      doesn't expose custom event variants, so we piggy-back on standard
      `output` events with `category: telemetry`, `output: "plc/cycleStats"`,
      and the structured payload in `data`
- [x] VS Code extension subscribes via `registerDebugAdapterTrackerFactory`
      (telemetry events don't surface through `onDidReceiveDebugSessionCustomEvent`)
- [x] `StatusBarItem` (Right alignment) renders:
      `$(pulse) PLC  142µs  #1,241  98µs/310µs  ●●`
- [x] Background → warning above 75% of watchdog, error above 100%
- [x] Click target: `structured-text.openMonitor`
- [x] Hide the StatusBarItem when no `st`-type debug session is active

**Interactive Continue + configurable cycle time** (added in this session — was
implicit in Tier 1 design but became its own work item):
- [x] `engine.cycle_time` parsed from `plc-project.yaml` via
      `EngineProjectConfig::from_project_yaml` (st-comm-api)
- [x] `Engine::run` honors `EngineConfig.cycle_time` — sleeps `target - elapsed`
      after each cycle so wall time matches the configured period
- [x] DAP session loads `engine.cycle_time` at launch and enforces the same
      period in its run loop, with `interruptible_sleep` (10ms chunks polling
      the request channel)
- [x] **Removed the 100k-cycle hard cap** in DAP Continue mode so debug
      sessions match every other PLC IDE: Continue runs until the user pauses,
      sets a breakpoint, or disconnects. A 10M-cycle safety net guards against
      runaway loops in tests
- [x] DAP run loop is interruptible: dedicated reader thread + mpsc channel,
      `process_inflight_requests` drains the channel between cycles, Pause /
      Disconnect / SetBreakpoints take effect mid-run, all other requests are
      queued and processed after `resume_execution` returns

**Tier 3 — dedicated "PLC Scan Cycle" tree view**
- [ ] `contributes.views` under the `debug` view container
- [ ] `TreeDataProvider` fed from the same `plc/cycleStats` events
- [ ] Rows: cycle count, last/min/max/avg, watchdog margin, instructions/cycle,
      per-device leaves (●/○ connected, last RTT)

**Tier 4 — CodeLens + watchdog Diagnostic**
- [ ] CodeLens above each `PROGRAM` / `FUNCTION_BLOCK` / `FUNCTION` header
      showing `⏱ Nµs last · Mµs max` (program-level only until Tier 6 lands)
- [ ] Watchdog budget read from `plc-project.yaml` (`engine.watchdog_ms`)
- [ ] When `last_us > budget`, push `DiagnosticSeverity.Warning` onto the POU
      header line so it shows in the Problems panel + as a squiggle

**Tier 5 — MonitorPanel sparkline**
- [ ] Add a "Cycle time" card to `editors/vscode/src/monitorPanel.ts`
- [ ] Rolling sparkline (last 300 cycles), histogram (10µs buckets), max/
      watchdog markers — sourced from `plc/cycleStats` telemetry

**Tier 6 — per-POU profiling (stretch)**
- [ ] VM tracks per-POU `call_count` + `total_time_ns` keyed by function index
- [ ] DAP custom event `plc/poStats` carries the table
- [ ] CodeLens upgraded to per-POU timing
- [ ] MonitorPanel "Top POUs by time" table

**Tier 7 — watchdog breakpoint (stretch)**
- [ ] `launch.json` option `"breakOnWatchdog": true`
- [ ] DAP emits `Stopped { reason: "exception", description: "watchdog ..." }`
      on overrun, dropping the user into the offending frame

**Cycle period + jitter tracking** (added in this session):
- [x] `CycleStats` gains `last_cycle_period`, `min_cycle_period`,
      `max_cycle_period`, `jitter_max` (period = wall-clock between consecutive
      cycle starts; cycle time = pure VM execution)
- [x] `Engine.run_one_cycle` measures the period via a `previous_cycle_start`
      Instant and updates `jitter_max = max(|period - target|)`
- [x] DAP mirrors the same tracking via `step_one_dap_iteration`, with
      `previous_cycle_start` reset on Halt so user think-time doesn't pollute
      the measurement
- [x] `scanCycleInfo` REPL shows period + jitter
- [x] `plc/cycleStats` telemetry payload (schema v2+) carries `target_us`,
      `last_period_us`, `min_period_us`, `max_period_us`, `jitter_max_us`
- [x] Status bar tooltip shows target/period/jitter when a `cycle_time` is set
- [x] Engine + DAP tests assert period stats are populated and jitter stays
      small relative to the target
- [x] Documentation: `cli/project-configuration.md` "Jitter" section explains
      what jitter is, where it's surfaced, and how to interpret it for
      time-sensitive control loops

**`avg_cycle_time` overflow + scope_refs leak fixes** (added in this session):
- [x] **Bug**: `avg_cycle_time` cast `cycle_count: u64` to `u32` for the
      Duration division, wrapping after 4.29 billion cycles (~71 minutes at
      1µs/cycle) — fixed via u128 division. Regression test added.
- [x] **Leak**: `scope_refs` HashMap in DapSession grew unboundedly across
      pause/resume cycles — fixed by clearing on `resume_execution` per DAP
      spec ("variable references invalid after Continued").

**`Continue` no longer freezes the play/pause button** (added in this session):
- [x] **Bug**: `handle_request` for Continue called the blocking
      `resume_execution` then returned the response. VS Code never received
      the Continue response in time to flip the play button to pause.
- [x] **Fix**: `is_resume_command()` detection in `run_dap`'s main loop sends
      the Continue/Step response and flushes it BEFORE entering the run loop.
      VS Code transitions to "running" state immediately; the yellow highlight
      clears.

**Live event streaming during Continue** (added in this session):
- [x] **Bug**: telemetry events (and any other `pending_events`) only got
      flushed to the wire AFTER `resume_execution` returned, so the status
      bar and Monitor panel were frozen during long Continue runs.
- [x] **Fix**: `resume_execution` now takes a `writer: &mut DapWriter<W>`
      parameter and drains `pending_events` to the wire on every cycle
      completion inside the loop.
- [x] `cycle_event_interval` is now computed from `engine.cycle_time` to
      target ~500ms between updates regardless of cycle period (10ms cycle →
      every 50, 100ms cycle → every 5, etc.). Free-run defaults to every 20.

**Integer width tracking + two's complement overflow** (added in this session):
- [x] **Bug**: SINT/INT/DINT declared variables were stored as raw i64 and
      never narrowed — a `cycle : SINT` counter grew to 1750 instead of
      wrapping at 127→-128. Also, the Monitor panel showed "INT" for all
      signed integer types (SINT/INT/DINT/LINT all collapsed to VarType::Int).
- [x] **Fix (IR)**: new `IntWidth` enum (I8/U8/I16/U16/I32/U32/I64/U64/None)
      added to `VarSlot` with `#[serde(default)]` for backward compat.
      Helper `IntWidth::display_name()` returns "SINT"/"USINT"/"DINT" etc.
- [x] **Fix (compiler)**: `int_width_from_ast()` maps every `ElementaryType`
      to its `IntWidth`. All VarSlot creation sites propagate the width.
- [x] **Fix (VM)**: `narrow_value(val, width)` applies two's complement
      wrapping at every store boundary: `local_set`, `set_global`,
      `set_global_by_slot`, `StoreGlobal` instruction, `force_variable`.
      Add/Sub/Mul switched to `i64::wrapping_*` so debug builds don't panic.
- [x] **Fix (display)**: `format_var_type_with_width()` returns "SINT"/
      "USINT"/"DINT" etc. Monitor panel and watch list show correct types.
- [x] 8 regression tests: SINT local/global wrap, USINT wrap, INT wrap,
      DINT no-wrap, comm `set_global_by_slot` narrowing, forced-value
      narrowing, cross-variant normalization (Int→UInt for ULINT slots)

**Literal context typing (semantic checker)** (added in this session):
- [x] `cycle : SINT := 0` and `cycle := cycle + 1` no longer error —
      the checker now allows integer literals to narrow to the assignment
      target when the value fits (matching Codesys/TwinCAT behavior)
- [x] `integer_type_range()`, `literal_fits_in_target()`, and
      `integer_literal_value()` helpers in st-semantics
- [x] `check_binary` does literal context typing: `cycle + 1` types as
      SINT (not INT) when `cycle` is SINT and 1 fits in SINT range
- [x] Out-of-range literals still error (e.g. `x : SINT := 200` → error)
- [x] 3 semantic tests: the user's exact program, in-range multi-type,
      out-of-range rejection

**Force variable improvements** (added in this session):
- [x] **Real PLC force semantics**: `forced_global_slots: HashSet<u16>`
      on Vm. `set_global_by_slot`, `set_global`, `StoreGlobal` all skip
      writes to forced slots. `force_variable` writes the forced value
      INTO the slot so every reader sees it naturally.
- [x] **Forced values narrowed**: typing 200 into a SINT force field
      stores -56 (the actual two's complement representation)
- [x] **Monitor panel**: forced rows show 🔒 lock icon + orange value;
      `forced: bool` per variable in telemetry; immediate telemetry push
      on force/unforce so the panel updates instantly
- [x] **Type validation**: force input validates against the variable's
      declared type (BOOL: true/false/0/1; INT: signed decimal; REAL:
      float; STRING: anything). Invalid input flashes red with a tooltip.
- [x] **Inflight evaluate**: `process_inflight_requests` handles
      `Evaluate` inline so addWatch/force/unforce take effect mid-run
- [x] 4 force regression tests + 1 DAP test for multi-var freeze scenario

**Global variable initialization** (added in this session):
- [x] **Bug**: `VAR_GLOBAL counter : USINT := 250;` was silently ignored —
      the compiler's `GlobalVarDeclaration` handler didn't emit init code,
      so all globals started at `Value::default_for_type` (0/false/empty).
- [x] **Fix**: compiler collects `(slot, init_expr)` pairs during pass 1,
      then generates a synthetic `__global_init` function containing one
      `StoreGlobal` per initializer. The engine calls `vm.run_global_init()`
      once at construction time.
- [x] 24 tests covering every elementary type (BOOL, SINT..ULINT, BYTE..
      LWORD, REAL, LREAL, STRING), multiple mixed-type declarations,
      overflow narrowing in init, init-runs-before-first-cycle,
      init-does-not-re-run, no-init-is-noop
- [x] User-defined type tests: TYPE alias to INT/REAL (works), ENUM with
      integer literal (works), ENUM by variant name (ignored — compiler
      doesn't resolve enum names in initializers yet), STRUCT field
      defaults (ignored — struct fields not materialized at runtime),
      ARRAY (ignored — array elements not materialized at runtime)

#### Phase 13a.3: Live Variable Monitor + Siemens-Style Watch Tables

Goal: a Codesys/TwinCAT/TIA Portal-grade variable monitor that streams live
values during a debug session, with a user-managed watch list that scales
to projects with hundreds of I/O points.

**Subscription model + watch list** (this session):
- [x] DAP `DapSession.watched_variables: Vec<String>` — telemetry only ships
      values for variables in this list, so projects with hundreds of I/O
      points don't waste 100KB/s on unused data
- [x] Evaluate REPL commands: `addWatch <var>`, `removeWatch <var>`,
      `watchVariables a,b,c`, `clearWatch`, `varCatalog`
- [x] Each watch mutation triggers an immediate `push_cycle_stats_event` so
      the panel updates instantly without waiting for the next 500ms tick
- [x] `Vm::monitorable_catalog()` enumerates globals + every PROGRAM POU's
      declared locals from the **module schema** (not runtime state), so the
      catalog is complete even before the first cycle has run
- [x] `plc/varCatalog` telemetry event pushed once per launch with
      `[{name, type}, ...]`. Used by the Monitor panel to populate its
      autocomplete dropdown
- [x] `Vm::monitorable_variables()` returns globals + PROGRAM retained locals
      namespaced as `Main.x`, `Pump.speed`, etc. (PROGRAM locals persist
      across cycles like globals do, conceptually)

**Monitor panel UX** (this session):
- [x] Full rewrite using `postMessage` for incremental DOM updates → zero
      flicker at 500ms refresh rate
- [x] Watch list table with autocomplete add input, per-row Force input, per-row
      Remove button, "Clear all" button
- [x] HTML5 `<datalist>` autocomplete sourced from the catalog — type a few
      letters of any variable, suggestions appear, hit Enter to add
- [x] Per-workspace persistence via `vscode.ExtensionContext.workspaceState`
      keyed on workspace folder path. Watch list survives panel close,
      window reload, and VS Code restart
- [x] On a new debug session, the persisted watch list is re-issued to the
      DAP via `watchVariables a,b,c`
- [x] Force/Unforce buttons wire to the DAP's existing
      `evaluate("force x = 42")` REPL — no more `prompt()` modal
- [x] Live cycle stats (cycles, last/min/max/avg, target, period, jitter) —
      updated from the same `plc/cycleStats` events at ~500ms cadence
- [x] Tests: `test_watch_list_flow` (addWatch round-trip),
      `test_var_catalog_emitted_on_launch` (catalog presence + content)

**Siemens TIA Portal-style Watch Tables** (future):

In TIA Portal, a "Watch table" is a named collection of variables with
per-row metadata (display format, comment, modify value, trigger). Users
create multiple tables for different subsystems (e.g. "Pumps", "Conveyor",
"Diagnostics") and switch between them in tabs. This is the gold standard
for industrial PLC monitoring and the natural next step after the basic
watch list.

- [ ] **Multiple named watch tables**: rename "Watch List" to "Watch Tables"
      with a tab strip at the top. Each tab is a named table (default: "Main")
- [ ] **Per-table persistence**: storage key changes from
      `plcMonitor.watchList:<workspace>` to
      `plcMonitor.watchTables:<workspace>` with the value being
      `{ tables: { "<name>": WatchTable }, activeTable: "<name>" }`
- [ ] **WatchTable schema**:
      ```ts
      interface WatchTableEntry {
        name: string;             // ST variable name (e.g. "io_rack.DI_0")
        comment?: string;         // user-supplied annotation
        displayFormat?: "dec" | "hex" | "bin" | "bool" | "ascii" | "float";
        modifyValue?: string;     // pre-typed value for one-click force
        triggerExpression?: string; // optional: only show / capture when this is true
      }
      interface WatchTable {
        name: string;
        entries: WatchTableEntry[];
        description?: string;     // shown as a tooltip on the tab
      }
      ```
- [ ] **Comment column** in the table UI — editable inline, persisted
      immediately on blur
- [ ] **Display format selector** per row (dropdown: dec/hex/bin/bool/ascii/float).
      Format is applied client-side in the panel; the wire format stays
      decimal so we don't bloat the telemetry payload
- [ ] **Modify column**: per-row text input pre-loaded with `modifyValue`,
      so a one-click "Modify" button forces the variable to a known
      pre-configured value (e.g. always force a setpoint to 100). Useful
      for commissioning and step-test workflows
- [ ] **Tab strip**: New / Rename / Duplicate / Delete table operations,
      drag-to-reorder
- [ ] **Import / Export**: serialize tables to a `.plc-watch.json` file in
      the project root so teams can share watch tables in version control
- [ ] **Compatibility with TIA Portal**: import `.tww` (Watch Table) files
      via a small parser — converts Siemens addressing (e.g. `%MW100`) to
      our flat `device.field` namespace where possible
- [ ] **DAP wire protocol**: extend `watchVariables` to accept a richer
      payload (`watchVariablesV2 [{name, displayFormat}, ...]`) so the DAP
      knows about display preferences and can format values server-side
      for types where the panel can't (e.g. STRING, custom STRUCTs)
- [ ] **Trigger expressions** (advanced): evaluate a boolean ST expression
      between cycles; only sample the table when it's true. Lets users
      capture state at a specific moment without setting a breakpoint
- [ ] **Snapshot / Compare**: button to capture the current values into a
      "snapshot" stored alongside the table. Side-by-side compare with
      live values, highlighting differences. Critical for regression
      testing controller behavior changes
- [ ] **Charting view**: secondary tab on each table that plots numeric
      variables over time using a sparkline / line chart. Reuses the
      cycle stats sparkline plumbing from Phase 13a.2 Tier 5
- [ ] Documentation: `docs/src/cli/watch-tables.md` quickstart with
      screenshots and a TIA-Portal-comparison cheat sheet

**Hierarchical FB instance display in debugger + Monitor panel** (future):

When the user watches a variable that is a FUNCTION_BLOCK instance (e.g.,
`counter : CTU` or `filler : FillController`), it should be displayed as a
collapsible tree — the parent node shows the instance name + type, and
expanding it reveals each field with its live value. This matches how
Codesys, TwinCAT, and TIA Portal display structured variables.

Currently, FB fields are shown flat (`counter.Q`, `counter.CV`, etc.). The
tree view is more natural for nested FBs like `filler.counter.Q` where
there are two levels of expansion.

Implementation plan:

- [x] **DAP: tree variables via `variablesReference`** (debugger Variables
      panel + Watch panel):
  - [x] FB instance locals are returned with `variablesReference > 0` (expandable)
        instead of being flattened. Scalar locals keep `variablesReference: 0`.
  - [x] `fb_var_refs: HashMap<i64, FbVarRef>` on DapSession maps ref IDs
        to `(caller_id, slot_idx, fb_func_idx)`. Cleared on `resume_execution`.
  - [x] Variables request with a FB ref returns the FB's fields as children.
        Nested FBs (e.g., CTU inside FillController) get their own refs
        for recursive expansion.
  - [x] Parent FB shows a summary value (e.g., `CU=TRUE, PV=5, Q=FALSE, CV=2`).
  - [x] Evaluate handler resolves dotted FB field paths (`ctr.Q`) via
        `resolve_fb_field` using the current frame's caller identity.
  - [x] 3 DAP integration tests:
    - `test_evaluate_fb_field_while_paused_inside_fb`: ctr.Q resolves, ctr
      has variablesReference > 0
    - `test_fb_instance_tree_expansion`: ctr is expandable with type "CTU"
    - `test_fb_children_request`: full round-trip — request ctr's children,
      verify CU/RESET/PV/Q/CV/prev_cu are returned as child Variables

- [x] **DAP Watch panel: expandable FB instances via Evaluate**:
  The VS Code Watch panel sends `evaluate` requests (not `variables`) when
  the user adds an expression. To make `filler` or `counter` expandable
  in the Watch panel, the Evaluate response needs `variablesReference > 0`.

  - [ ] Change `handle_evaluate` to detect when the expression resolves to
        a FB instance (check the resolved symbol's `VarType::FbInstance`).
        When it does, allocate a `variablesReference` and store a `FbVarRef`
        — the same mechanism used in `handle_variables` for the Locals panel.
  - [ ] Set `EvaluateResponse.variablesReference` to the allocated ref ID
        so VS Code shows the expand arrow.
  - [ ] The `result` field should show the summary value (same as
        `fb_summary_value` used in the Locals panel).
  - [ ] Test: add `counter` to Watch while paused inside FillController →
        Watch shows "CTU (CU=TRUE, CV=2, ...)" with expand arrow → expand
        shows Q, CV, PV, etc.

- [x] **Monitor panel: tree-view watch list**:
  - [ ] Change the watch list data model from flat `string[]` to a tree:
        ```ts
        interface WatchEntry {
          name: string;        // e.g. "Main.filler"
          path: string;        // full dotted path for the DAP
          children?: WatchEntry[];  // null for scalars, populated for FBs
          expanded?: boolean;
        }
        ```
  - [ ] When the user adds a variable that resolves to a FB type (the catalog
        includes type info), auto-query the catalog for its children and build
        the tree node. Use HTML `<details>/<summary>` elements or a custom
        toggle UI for expand/collapse.
  - [ ] Telemetry payload: when a watched variable is a FB instance, the DAP
        should send the parent entry + all child values in a nested structure
        rather than requiring the panel to request children separately.
        Proposed schema change:
        ```json
        {
          "name": "Main.filler.counter",
          "type": "CTU",
          "value": "CV=3, Q=TRUE",
          "children": [
            { "name": "CU", "value": "TRUE", "type": "BOOL" },
            { "name": "RESET", "value": "FALSE", "type": "BOOL" },
            { "name": "PV", "value": "5", "type": "INT" },
            { "name": "Q", "value": "TRUE", "type": "BOOL" },
            { "name": "CV", "value": "3", "type": "INT" }
          ]
        }
        ```
  - [ ] Persist expand/collapse state in the workspace state alongside the
        watch list so the tree structure survives panel close / reload.
  - [ ] When a FB instance has many children (e.g., a large IO module with
        dozens of fields), add a "Collapse all" / "Expand all" action.

- [ ] **Catalog enhancement**: the `plc/varCatalog` event should include
      a `children` list for each FB-typed entry so the panel knows the tree
      structure before any values arrive. The catalog already has `type` — add
      an optional `childNames: [{name, type}, ...]` field for FB entries.

- [ ] **Tests**:
  - [ ] DAP integration: tree expansion for single-level FB (CTU) and
        two-level nested FB (FillController → CTU)
  - [ ] Monitor panel: verify the tree renders with correct expand/collapse
  - [ ] Verify tree state persists across panel close / reload
  - [ ] Performance: verify that a FB with 50+ fields doesn't bloat the
        telemetry payload beyond reasonable size (only send children for
        expanded nodes, or cap at first-level expansion)

#### Phase 13b: Real Protocol Implementations

- [ ] **`st-comm-link-tcp` crate** (TCP link):
  - [ ] TCP socket management (connect, reconnect, timeout)
  - [ ] Implements `CommLink` trait
  - [ ] Unit tests with mock TCP listener
- [ ] **`st-comm-link-serial` crate** (serial link):
  - [ ] Serial port management (RS-485/RS-232, baud, parity, data bits, stop bits)
  - [ ] Implements `CommLink` trait
  - [ ] Unit tests with mock serial port / PTY pair
- [ ] **`st-comm-modbus` crate** (Modbus protocol — works over any link):
  - [ ] Implements `CommDevice` trait for Modbus
  - [ ] TCP framing: MBAP header (auto-selected when link is TCP)
  - [ ] RTU framing: CRC-16, silence detection (auto-selected when link is serial)
  - [ ] ASCII framing: LRC (optional, for serial links)
  - [ ] Read coils, discrete inputs, holding registers, input registers
  - [ ] Write single/multiple coils, single/multiple registers
  - [ ] Cyclic polling with configurable interval
  - [ ] Device profile field ↔ register mapping with scaling
  - [ ] Unit tests with mock link
  - [ ] Integration tests with Modbus simulator
- [ ] **Additional CLI commands**:
  - [ ] `st-cli comm-test` sends a test read to verify connectivity
  - [ ] `st-cli profile import` converts GSD/GSDML/ESI/EDS → YAML profile
- [ ] **Bundled hardware device profiles**:
  - [ ] Generic Modbus I/O (coils + registers, 8/16/32 channel variants)
  - [ ] ABB ACS580 VFD
  - [ ] Siemens G120 VFD
  - [ ] WAGO 750-352 I/O coupler
  - [ ] Generic temperature sensor (RTD/thermocouple via analog input)
- [ ] **Documentation**:
  - [ ] Communication architecture guide (link/device layering, multi-rate, diagnostics)
  - [ ] "Creating a Link Extension" tutorial
  - [ ] "Creating a Device Extension" tutorial
  - [ ] Modbus quickstart (TCP + RTU examples)

#### Phase 13c: Future Protocol Extensions (separate crates)

  - [ ] `st-comm-link-udp` — UDP link
  - [ ] `st-comm-profinet` — PROFINET I/O device extension
  - [ ] `st-comm-ethercat` — EtherCAT device extension
  - [ ] `st-comm-canopen` — CANopen / CAN bus device extension
  - [ ] `st-comm-opcua` — OPC UA client device extension
  - [ ] `st-comm-mqtt` — MQTT publish/subscribe device extension
  - [ ] `st-comm-s7` — Siemens S7 protocol device extension
  - [ ] `st-comm-ethernet-ip` — EtherNet/IP (Allen-Bradley) device extension

---
