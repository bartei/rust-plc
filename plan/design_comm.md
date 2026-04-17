# Communication Layer — Design Document

> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker (Phases 0-12).
> **Todo list:** [implementation_comm.md](implementation_comm.md) — progress tracking and actionable items.
> **See also:** [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets (Phase 14).
> **See also:** [design_deploy.md](design_deploy.md) — remote deployment & online management (Phase 15).

## Communication Extension System & Modbus Implementation

A PLC is only useful if it can talk to the physical world. This phase establishes the
**communication extension architecture** — a modular, plugin-based system where each
protocol (Modbus, Profinet, EtherCAT, etc.) is an independent, versioned extension —
and delivers the first two implementations: Modbus TCP and Modbus RTU/ASCII.

---

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

---

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

---

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

---

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

---

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

---

### Auto-Generated ST Types and Globals

The configuration auto-generates struct types from device profiles and named global instances.
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

---

### Simulated Device

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

---

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

---

### Device Description Import

Inspired by CODESYS's universal import capability. A CLI tool converts standard device
description files into our YAML profile format:

```bash
st-cli profile import --format gsdml    device.xml     # PROFINET
st-cli profile import --format esi      device.xml     # EtherCAT
st-cli profile import --format eds      device.eds     # CANopen / EtherNet/IP
```

This generates a `.yaml` profile with the struct fields and register mappings extracted
from the standard file. Users can then edit the generated YAML to customize field names,
or remove unused channels.

---

### Device Profile System

A device profile is a reusable YAML file that defines **both** the struct schema (fields
visible in ST code) **and** the register map (how the communication runtime reads/writes
the physical device). Profiles can be shared between projects and published as a community
library.

Each profile defines:
1. **Struct type name** — becomes the TYPE name in generated ST code
2. **Fields** — each field has a name, ST data type, direction, and register mapping
3. **Register mapping** — Modbus register address, type, bit position

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

  # Analog I/O
  - name: SPEED_REF
    type: REAL
    direction: output
    register: { address: 1, kind: holding_register }

  - name: SPEED_ACT
    type: REAL
    direction: input
    register: { address: 2, kind: input_register }

  - name: CURRENT
    type: REAL
    direction: input
    register: { address: 3, kind: input_register }

  - name: TORQUE
    type: REAL
    direction: input
    register: { address: 4, kind: input_register }

  - name: POWER
    type: REAL
    direction: input
    register: { address: 5, kind: input_register }
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

---

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

---

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

---

### Diagnostics Exposure (HMI / SCADA Integration)

Two-layer design — one ground truth (ST globals), one convenience layer (HTTP JSON).

**Layer 1 — diagnostics as auto-generated ST globals (ground truth)**

At `CommManager::register_device()`, reserve six diag globals per device using the
existing `{device}_{field}` flat-naming convention:

| Global | Type | Description |
|--------|------|-------------|
| `{device}_diag_connected` | BOOL | Responding this cycle |
| `{device}_diag_error` | BOOL | Last transaction failed |
| `{device}_diag_error_count` | UDINT | Cumulative errors |
| `{device}_diag_cycles_ok` | UDINT | Successful scan cycles |
| `{device}_diag_last_resp_ms` | UINT | Last round-trip time |
| `{device}_diag_last_update` | UDINT | Engine cycle of last good I/O |

After `write_outputs()` in the scan cycle, call `device.diagnostics()` and write the
six fields onto their reserved global slots. `_io_map.st` emits a trailing
`--- DIAGNOSTICS ---` block per device so LSP / semantic checker / user ST code all
see the diag globals.

Same treatment for links: `{link}_link_is_open`, `{link}_link_bytes_sent`,
`{link}_link_bytes_received`, `{link}_link_errors` (deferred until real `CommLink`
implementations exist in Phase 13b).

Engine-level globals: `engine_cycle_count`, `engine_last_cycle_us`,
`engine_min_cycle_us`, `engine_max_cycle_us`, `engine_avg_cycle_us`.

**Layer 2 — HTTP `/api/diagnostics` JSON endpoint (convenience layer)**

Separate port from the monitor WebSocket (HMIs and the monitor UI have different
auth/CORS profiles). Declared in `plc-project.yaml`:

```yaml
diagnostics:
  port: 9090
  bind: 127.0.0.1
```

Endpoints:
- `GET /api/diagnostics` — full snapshot with `"schema": "1"` for forward compatibility
- `GET /api/diagnostics/devices/{name}` — single device
- `GET /api/diagnostics/summary` — `{ healthy, device_count, connected_count, error_count }`

Response schema:
```json
{
  "schema": "1",
  "ts_ms": 1775692800123,
  "engine":  { "cycle_count": 0, "last_us": 0, "min_us": 0,
               "max_us": 0, "avg_us": 0 },
  "links":   { "<link>": { "is_open": true, "bytes_sent": 0 } },
  "devices": { "<device>": { "protocol": "...", "profile": "...",
                              "connected": true, "error": false,
                              "error_count": 0, "successful_cycles": 0,
                              "last_response_ms": 0, "last_error": null,
                              "last_update_cycle": 0 } }
}
```

**Layer 3 — documentation (`docs/comm/diagnostics.md`)**

Field-by-field reference, ST code examples, `/api/diagnostics` schema reference,
Node-RED quickstart (inject → http request → json → switch → notification),
FUXA quickstart (Web API device + tag bindings + connection panel).

---

### Real Protocol Implementations

**TCP link** (`st-comm-link-tcp`): TCP socket management with connect/reconnect/timeout.

**Serial link** (`st-comm-link-serial`): RS-485/RS-232 with baud/parity/data bits/stop bits.

**Modbus protocol** (`st-comm-modbus`): Works over any link. TCP framing (MBAP, auto-selected
for TCP links), RTU framing (CRC-16, for serial links), ASCII framing (LRC, optional).
Read/write coils, discrete inputs, holding registers, input registers. Cyclic polling
with configurable interval. Device profile field ↔ register mapping.

---

### Future Protocol Extensions

Each protocol is a separate crate:
- `st-comm-link-udp` — UDP link
- `st-comm-profinet` — PROFINET I/O
- `st-comm-ethercat` — EtherCAT
- `st-comm-canopen` — CANopen / CAN bus
- `st-comm-opcua` — OPC UA client
- `st-comm-mqtt` — MQTT publish/subscribe
- `st-comm-s7` — Siemens S7 protocol
- `st-comm-ethernet-ip` — EtherNet/IP (Allen-Bradley)