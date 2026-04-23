# Communication Layer — Design Document

> **Progress tracker:** [implementation_comm.md](implementation_comm.md)
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.

## Overview

The communication layer provides access to external devices (I/O racks, VFDs, sensors)
through standard IEC 61131-3 function block syntax. Device types are derived from YAML
profiles and exposed as callable function blocks in ST code:

```st
PROGRAM Main
VAR
    serial     : SerialLink;
    io_rack    : ModbusRtuIoRack;
    pump_vfd   : ModbusRtuVfd;
END_VAR
    serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);

    io_rack(link := serial.port, slave_id := 1, refresh_rate := T#50ms);
    pump_vfd(link := serial.port, slave_id := 2, refresh_rate := T#100ms);

    io_rack.DO_0 := io_rack.DI_0;

    IF io_rack.DI_6 AND pump_vfd.READY THEN
        pump_vfd.RUN := TRUE;
        pump_vfd.SPEED_REF := INT_TO_REAL(IN1 := io_rack.AI_3) * 0.005;
    END_IF;
END_PROGRAM
```

---

## Architecture

### Native Function Blocks (NativeFb)

Communication devices are implemented as **native function blocks** — Rust-backed FBs that
appear as normal `FUNCTION_BLOCK` types in the editor and debugger but execute native Rust
code instead of interpreted ST instructions.

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
| **Semantic analyzer** | Field names + types for completions, hover, type checking |
| **Compiler** | `MemoryLayout` for synthetic `Function` entry with correct locals |
| **VM** | Field slice for `execute()` dispatch |
| **LSP** | Same symbol table — dot-completion, hover, go-to-definition |
| **DAP** | Same `Function.locals` — variable expansion, watch, force/unforce |
| **Target agent** | Same registry rebuilt from bundled profiles on deploy + reboot |

### NativeFbRegistry

A central registry holds all available native FB types. Built from device profiles at
startup and passed through the full pipeline:

1. `analyze_with_native_fbs()` — injects FB types into symbol table
2. `compile_with_native_fbs()` — creates synthetic Function entries in Module
3. `Vm::new_with_native_fbs()` — dispatches `CallFb` to `execute()`

The registry is built in every context that needs it: CLI, LSP, DAP, bundle creation,
target agent (from persisted profiles), and auto-start after reboot.

### Forced Variable Support

Native FB fields support the PLC force mechanism. After `execute()` returns, any fields
that are in the `forced_variables` map are overwritten with the forced value before the
state is saved back to `fb_instances`. This ensures forced values persist even when
`execute()` would normally overwrite them from hardware state.

---

## Two-Layer Model

### Links — Physical Transport

A **link** is a native FB representing a physical communication channel. It owns the
OS resource (serial port, TCP socket) and provides send/receive primitives.

| Link Type | VAR_INPUT Parameters | Transport |
|-----------|---------------------|-----------|
| `SerialLink` | port, baud, parity, data_bits, stop_bits | RS-232/RS-485 serial |
| `SimulatedLink` | *(none)* | In-memory (for testing) |

Link FBs are called once per cycle. On first call, they open the connection. On
subsequent calls, they maintain the connection and set `connected := TRUE/FALSE`.
The link's `execute()` does not perform any protocol I/O — it just manages the
transport layer.

> **Note:** Modbus TCP does not use a separate link FB — the device FB owns its TCP
> connection directly. See [Modbus TCP Protocol](#modbus-tcp-protocol) below.

### Devices — Protocol Endpoints

A **device** is a native FB that implements a protocol (Modbus RTU, Modbus TCP, etc.)
and is parameterized by a YAML device profile (register map).

| Device Type | Protocol | Transport | Link? |
|-------------|----------|-----------|-------|
| `ModbusRtuDevice` | Modbus RTU over serial | `SerialLink` | Yes — shared bus |
| `ModbusTcpDevice` | Modbus TCP over Ethernet | Built-in TCP | No — point-to-point |
| `SimulatedDevice` | In-memory registers + web UI | None | No |

Modbus RTU devices take a `link` parameter referencing a SerialLink (shared half-duplex
bus). Modbus TCP devices own their connection directly via `host`/`port` parameters
(point-to-point, no bus sharing needed).

The device's `execute()`:
1. Checks if `refresh_rate` interval has elapsed (multi-rate scheduling)
2. Uses the transport to send protocol requests and receive responses
3. Maps register values to/from the FB's field slots via the profile

### Link-Device Binding

The device takes the link as a `link : INT` parameter. The compiler passes the link
instance's slot index as an integer. At runtime, the device's `execute()` uses this
handle to look up the link's shared transport state.

```st
serial(port := '/dev/ttyUSB0', baud := 9600);         (* opens the port *)
io_rack(link := serial.port, slave_id := 1);                (* uses serial's transport *)
pump_vfd(link := serial.port, slave_id := 2);               (* shares the same port *)
```

Multiple devices can share a single link. The link's `execute()` manages bus access
coordination (mutex-based for RS-485 half-duplex).

---

## Device Profiles

YAML files define the register map and field schema for specific hardware:

```yaml
name: WagoIO16
vendor: WAGO
protocol: modbus-rtu
description: "WAGO 750-352 16-channel digital I/O"
fields:
  - name: DI_0
    type: BOOL
    direction: input
    register: { address: 0, kind: discrete_input, bit: 0 }
  - name: DO_0
    type: BOOL
    direction: output
    register: { address: 0, kind: coil, bit: 0 }
  - name: AI_0
    type: INT
    direction: input
    register: { address: 0, kind: input_register, scale: 0.1, unit: mA }
```

`DeviceProfile::to_native_fb_layout()` converts a profile into a `NativeFbLayout`:
- `link : INT` — link instance handle (VarInput)
- `slave_id : INT` — Modbus slave address (VarInput)
- `refresh_rate : TIME` — polling interval (VarInput)
- Diagnostic fields: `connected`, `error_code`, `io_cycles`, `last_response_ms` (Var)
- All profile I/O fields (Var — readable and writable from ST)

### Field Mapping

| Profile direction | FB var kind | ST access |
|-------------------|-------------|-----------|
| input | Var | Read via dot notation (`dev.DI_0`) |
| output | Var | Read/write via dot notation (`dev.DO_0 := TRUE`) |
| inout | Var | Read/write via dot notation |

All I/O fields use Var (not VarOutput) so the user program can both read and write them.

### Register Types

| Kind | Modbus Function | Access | Typical Use |
|------|----------------|--------|-------------|
| `coil` | FC01/FC05/FC15 | Read/Write | Digital outputs |
| `discrete_input` | FC02 | Read only | Digital inputs |
| `holding_register` | FC03/FC06/FC16 | Read/Write | Analog outputs, config |
| `input_register` | FC04 | Read only | Analog inputs, measurements |
| `virtual` | N/A | In-memory | Simulated devices |

---

## RS-485 Serial Link

### Hardware Model

RS-485 is a half-duplex differential serial bus. Multiple devices share the same
physical wire pair. Only one device transmits at a time (master-slave model).

```
[PLC] ──── RS-485 Bus ──┬── [Slave 1: I/O Rack]
                         ├── [Slave 2: VFD]
                         └── [Slave 3: Sensor]
```

### SerialLink NativeFb

```
FUNCTION_BLOCK SerialLink
VAR_INPUT
    port       : STRING;    (* '/dev/ttyUSB0', '/dev/ttyAMA0' for RPi *)
    baud       : INT;       (* 9600, 19200, 38400, 57600, 115200 *)
    parity     : STRING;    (* 'N', 'E', 'O' *)
    data_bits  : INT;       (* 7, 8 *)
    stop_bits  : INT;       (* 1, 2 *)
END_VAR
VAR
    connected  : BOOL;
    error_code : INT;
END_VAR
END_FUNCTION_BLOCK
```

**Implementation (`execute()`):**
- First call: open serial port with the configured parameters, store file descriptor
- Subsequent calls: verify port is still open, reconnect if lost
- Connection state shared with device FBs via the link handle
- The link itself does NOT perform reads/writes — devices do that via the shared transport
- Bus access coordination: mutex ensures only one device transmits at a time

### Raspberry Pi Support

The Raspberry Pi exposes UART via `/dev/ttyAMA0` (GPIO 14/15) or `/dev/ttyUSB0` (USB
RS-485 adapter). Both work with the standard `serialport` Rust crate. For RS-485
direction control (DE/RE pin), the Linux kernel's `RS485` ioctl handles it
automatically when the serial driver supports it.

---

## Modbus RTU Protocol

### Protocol Overview

Modbus RTU is a binary serial protocol. The master (PLC) sends requests, each slave
responds. Frames are delimited by 3.5-character silent intervals.

**Frame format:**
```
[Slave Address: 1 byte] [Function Code: 1 byte] [Data: N bytes] [CRC16: 2 bytes]
```

### ModbusRtuDevice NativeFb

A generic Modbus RTU device parameterized by a YAML profile. One Rust implementation
handles any Modbus RTU device — the profile defines which registers to read/write.

```
FUNCTION_BLOCK ModbusRtuDevice
VAR_INPUT
    link         : INT;     (* SerialLink handle *)
    slave_id     : INT;     (* Modbus slave address 1-247 *)
    refresh_rate : TIME;    (* Polling interval *)
END_VAR
VAR
    connected      : BOOL;
    error_code     : INT;
    io_cycles      : UDINT;
    last_response_ms : REAL;
    (* ... profile fields generated from YAML ... *)
END_VAR
END_FUNCTION_BLOCK
```

**Implementation (`execute()`):**

1. **Timing check:** If `refresh_rate` hasn't elapsed since last I/O, return early
2. **Read inputs:** For each input-direction field in the profile:
   - Build a Modbus read request (FC01/FC02/FC03/FC04 based on register kind)
   - Send via the link's serial port (acquire bus mutex first)
   - Parse response, apply scaling/offset, write to field slot
3. **Write outputs:** For each output-direction field:
   - Read current value from field slot
   - Build a Modbus write request (FC05/FC06/FC15/FC16)
   - Send via the link's serial port
4. **Update diagnostics:** `connected`, `error_code`, `io_cycles`, `last_response_ms`

### Register Grouping Optimization

Instead of one Modbus transaction per field, group consecutive registers into single
multi-register read/write requests:
- `FC01` Read Coils: read multiple coils in one request
- `FC03` Read Holding Registers: read a contiguous range
- `FC16` Write Multiple Registers: write a contiguous range

The profile's register addresses determine grouping. Non-contiguous registers require
separate transactions.

### Error Handling

| Error | Behavior |
|-------|----------|
| No response (timeout) | `connected := FALSE`, `error_code := 1`, retry next cycle |
| CRC mismatch | `error_code := 2`, discard frame, retry |
| Modbus exception response | `error_code := exception_code + 100`, log details |
| Serial port error | `connected := FALSE`, `error_code := 3`, link reconnects |

### Timing Constraints

Modbus RTU requires 3.5-character silent intervals between frames. At 9600 baud:
- 1 character = 11 bits (start + 8 data + parity + stop) = ~1.15ms
- 3.5 characters = ~4ms minimum inter-frame gap
- The serial link's `execute()` enforces this timing

---

## Deployment Pipeline

### Bundle Inclusion

Device profiles are included in the `.st-bundle` archive alongside the compiled bytecode.
The bundle creation searches for profiles in:
1. `{project_root}/profiles/`
2. Parent directories up to 6 levels (workspace root pattern)

### Target Agent Integration

When a bundle is uploaded to the target agent:
1. Profiles are persisted to `current_profiles/` on disk
2. When the program starts (API or auto-start after reboot), profiles are loaded
3. A `NativeFbRegistry` is built with `SimulatedNativeFb` instances (or real protocol
   FBs once implemented)
4. The registry is passed to `Engine::new_with_native_fbs()`

This ensures `execute()` runs on the target, diagnostic fields update, and force/unforce
works — all surviving reboots via the systemd service + auto-start.

### E2E Verified

The following is verified by QEMU e2e tests on both x86_64 and aarch64:
- Bundle includes profiles from workspace root
- Agent persists profiles on upload
- Program starts with correct `cycle_time` from `plc-project.yaml`
- `execute()` runs (`connected=TRUE`, `io_cycles` advances)
- Force DI_0=TRUE → program logic → DO_0=TRUE (I/O flow verified)
- All survives VM reboot (auto-start, registry rebuild, force/unforce)

---

## Simulated Device

`SimulatedNativeFb` wraps a `SimulatedDevice` with an `Arc<Mutex<HashMap<String, IoValue>>>`
shared state. Used for development/testing without hardware.

**`execute()` method:**
1. Reads input-direction fields from shared state → writes to FB field slots
2. Reads output-direction fields from FB field slots → writes to shared state
3. Updates diagnostic fields (connected, io_cycles, etc.)

The shared state is accessible via:
- **Web UI** (HTTP + WebSocket): toggle inputs, observe outputs in browser
- **Variables API**: read/write via `GET/POST /api/v1/variables`
- **Force mechanism**: forced values survive `execute()` calls

---

## Modbus TCP Protocol

### Protocol Overview

Modbus TCP is a TCP/IP variant of the Modbus protocol. Unlike RTU (shared serial bus),
each TCP connection is point-to-point — one connection per remote device. No bus
coordination or inter-frame timing is needed.

**Frame format (MBAP — Modbus Application Protocol header):**
```
[Transaction ID: 2B] [Protocol ID: 2B = 0x0000] [Length: 2B] [Unit ID: 1B] [PDU...]
```

No CRC is needed — TCP handles data integrity. The PDU (function code + data) is
identical to RTU.

### Architecture

Unlike Modbus RTU (which uses a two-layer model with shared SerialLink + BusManager),
Modbus TCP uses a **unified model**: each device FB owns its own TCP connection and
background I/O thread directly. This is simpler because TCP connections are
point-to-point — no bus sharing to coordinate.

```
[PLC] ──── TCP ──── [Device 1: 192.168.1.100:502]
      ──── TCP ──── [Device 2: 192.168.1.101:502]
```

Implementation: `st-comm-modbus-tcp` crate (self-contained, independent of
`st-comm-serial` and `st-comm-modbus`).

### ModbusTcpDevice NativeFb

```
FUNCTION_BLOCK ModbusTcpDevice
VAR_INPUT
    host           : STRING;    (* '192.168.1.100' *)
    port           : INT;       (* 502 — default Modbus TCP port *)
    unit_id        : INT;       (* Modbus unit identifier *)
    refresh_rate   : TIME;      (* Polling interval *)
END_VAR
VAR
    connected        : BOOL;
    error_code       : INT;
    io_cycles        : UDINT;
    last_response_ms : REAL;
    (* ... profile fields generated from YAML ... *)
END_VAR
END_FUNCTION_BLOCK
```

**Implementation (`execute()`):**

1. **First call:** Spawn a dedicated background I/O thread with a TCP connection
2. **Background thread:** Connect → loop { read batched inputs, write batched outputs,
   update diagnostics, sleep for `refresh_rate` }. Auto-reconnect on connection failure.
3. **Subsequent calls:** Copy cached read values from IoState, queue write values
   (non-blocking — never blocks the scan cycle)

### Usage

```st
PROGRAM Main
VAR
    io : Waveshare8RelayOutput;
END_VAR
    io(host := '10.1.2.133', port := 502, unit_id := 1, refresh_rate := T#50ms);
    io.DO0 := TRUE;
END_PROGRAM
```

Device profiles use `protocol: modbus-tcp` and share the same register map format as
RTU profiles.

### Register Grouping

Same optimization as RTU: consecutive registers are batched into single multi-register
read/write requests. Consecutive coils use FC0F (Write Multiple Coils) instead of
individual FC05 calls.

---

## Plugin System (Planned)

**Tier 1 — Device profile plugins:**
- Git repos containing YAML profiles + optional ST library code
- Referenced in `plc-project.yaml` under a `plugins:` section
- Managed via `st-cli plugin fetch/update/list`
- No binary recompilation needed

**Tier 2 — Protocol plugins:**
- New protocols require Rust implementation in the core project
- Each protocol is a generic NativeFb parameterized by the profile
- RS-485/Modbus RTU is the first real protocol (see above)

---

## Connection Lifecycle

- **First call:** Parameters latched, connection opened (port/socket)
- **Subsequent calls:** Idempotent on config; perform scheduled I/O
- **Connection loss:** `connected := FALSE`, retry with backoff
- **Refresh rate:** Handled inside `execute()` with internal timing
- **Forced fields:** Re-applied after every `execute()` call
