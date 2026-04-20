# Device Profiles

A device profile is a YAML file that describes a hardware device's registers —
what fields it exposes, their data types, I/O direction, and how they map to
protocol registers (Modbus addresses, coils, etc.).

## Profile location

Place profile files in your project's `profiles/` directory:

```
my-project/
  plc-project.yaml
  main.st
  profiles/
    my_io_module.yaml
    my_vfd.yaml
```

The runtime also searches parent directories (up to 6 levels) for a `profiles/`
folder, so you can share profiles across projects in a workspace.

## Profile format

```yaml
name: MyIoModule                    # Type name in ST code (required)
vendor: ACME Corp                   # Optional
protocol: modbus-rtu                # Protocol: modbus-rtu, simulated
description: "16-channel I/O"       # Optional

fields:
  - name: DI_0                      # Field name in ST code
    type: BOOL                      # IEC 61131-3 data type
    direction: input                # input, output, or inout
    register:
      address: 0                    # Modbus register address (0-based)
      kind: discrete_input          # Register type (see below)

  - name: DO_0
    type: BOOL
    direction: output
    register:
      address: 0
      kind: coil

  - name: AI_0
    type: INT
    direction: input
    register:
      address: 0
      kind: input_register
      scale: 0.1                    # Optional: ST_value = raw * scale
      offset: 0.0                   # Optional: ST_value = raw * scale + offset
      unit: mA                      # Optional: documentation only
```

## Field types

All standard IEC 61131-3 types are supported:

| Type | Size | Range | Typical use |
|------|------|-------|-------------|
| `BOOL` | 1 bit | TRUE/FALSE | Digital I/O |
| `SINT` | 8-bit signed | -128..127 | Small integers |
| `INT` | 16-bit signed | -32768..32767 | Analog I/O (raw) |
| `DINT` | 32-bit signed | -2^31..2^31-1 | Counters, accumulators |
| `LINT` | 64-bit signed | -2^63..2^63-1 | Large counters |
| `USINT` | 8-bit unsigned | 0..255 | Status bytes |
| `UINT` | 16-bit unsigned | 0..65535 | Analog I/O (raw) |
| `UDINT` | 32-bit unsigned | 0..2^32-1 | Timers, large counters |
| `ULINT` | 64-bit unsigned | 0..2^64-1 | Very large counters |
| `REAL` | 32-bit float | ±3.4e38 | Temperature, speed, etc. |
| `LREAL` | 64-bit float | ±1.8e308 | High-precision measurements |
| `BYTE` | 8-bit | 0..255 | Bit patterns, flags |
| `WORD` | 16-bit | 0..65535 | Raw register values |
| `DWORD` | 32-bit | 0..2^32-1 | 32-bit bit patterns |
| `LWORD` | 64-bit | 0..2^64-1 | 64-bit bit patterns |
| `STRING` | variable | — | Text values |
| `TIME` | 64-bit ms | — | Durations (e.g., T#50ms) |

## Field direction

| Direction | Meaning | ST access |
|-----------|---------|-----------|
| `input` | Device → PLC (read from device) | Read via `dev.field` |
| `output` | PLC → Device (written to device) | Read/write via `dev.field` |
| `inout` | Bidirectional | Read/write via `dev.field` |

All fields are accessible via dot notation in ST code regardless of direction.
The direction controls which Modbus function codes are used:
- **Input fields**: read with FC01/FC02/FC03/FC04
- **Output fields**: written with FC05/FC06/FC0F/FC10

## Register types

| Kind | Modbus FC | Access | Typical use |
|------|-----------|--------|-------------|
| `coil` | FC01 read / FC05 write | Read/Write | Digital outputs |
| `discrete_input` | FC02 read | Read only | Digital inputs |
| `holding_register` | FC03 read / FC06 write | Read/Write | Analog outputs, config |
| `input_register` | FC04 read | Read only | Analog inputs, measurements |
| `virtual` | N/A | In-memory | Simulated devices (testing) |

## Register mapping options

Each field's `register` section supports these options:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `address` | integer | **required** | Register address (0-based). Range: 0–65535 |
| `kind` | string | **required** | Register type: `coil`, `discrete_input`, `holding_register`, `input_register`, `virtual` |
| `bit` | integer | — | Bit position (0–15) for BOOL fields packed into a word register |
| `scale` | float | — | Scaling factor: `ST_value = raw * scale` |
| `offset` | float | — | Offset after scaling: `ST_value = raw * scale + offset` |
| `unit` | string | — | Engineering unit for documentation (e.g., `"°C"`, `"Hz"`, `"mA"`) |
| `byte_order` | string | `big-endian` | Byte order: `big-endian` or `little-endian` |
| `word_count` | integer | 1 | Number of 16-bit registers to read (use 2 for 32-bit DINT/REAL values) |

## Register scaling

For analog values, you can define scaling and offset:

```yaml
- name: TEMPERATURE
  type: REAL
  direction: input
  register:
    address: 10
    kind: input_register
    scale: 0.1        # raw register value × 0.1
    offset: -40.0     # ... then add -40
    unit: "°C"
```

With this profile, a raw register value of `450` becomes `450 × 0.1 + (-40) = 5.0°C`.

For output fields, the inverse is applied when writing:
`raw = (ST_value - offset) / scale`

## Two-layer model

The communication architecture separates **transport** from **protocol**:

- **SerialLink** manages the physical serial port (open, configure, reconnect)
- **Device FBs** (from profiles) handle the protocol (Modbus RTU) using the link

Multiple devices can share one serial link. The link ensures only one device
talks on the bus at a time (RS-485 half-duplex coordination).

```st
VAR
    serial   : SerialLink;           (* Transport layer *)
    io_rack  : MyIoModule;           (* Protocol layer — device 1 *)
    pump_vfd : MyVfd;                (* Protocol layer — device 2 *)
END_VAR
    serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);
    io_rack(link := serial.port, slave_id := 1, refresh_rate := T#50ms);
    pump_vfd(link := serial.port, slave_id := 2, refresh_rate := T#100ms);
```

## Generated FB layout

When the runtime loads a `modbus-rtu` profile, it creates a function block type
with the following fields in order:

1. **Link binding** (VAR_INPUT):
   - `link : STRING` — serial port path from a SerialLink instance (e.g., `serial.port`)

2. **Modbus parameters** (VAR_INPUT):
   - `slave_id : INT` — Modbus slave address (1–247)
   - `refresh_rate : TIME` — how often to poll the device (e.g., `T#50ms`)

3. **Diagnostic fields** (VAR):
   - `connected : BOOL` — TRUE if the device is responding
   - `error_code : INT` — 0 = OK (see [error codes](#error-codes))
   - `io_cycles : UDINT` — number of successful I/O cycles
   - `last_response_ms : REAL` — last round-trip time in milliseconds

4. **I/O fields** (VAR) — one per field in the profile, in declaration order

### SerialLink layout

The `SerialLink` function block manages the physical serial port. It is called
once per scan cycle. On first call it opens the port; on subsequent calls it
reports cached connection state without blocking the scan cycle.

| Field | Type | Kind | Description |
|-------|------|------|-------------|
| `port` | STRING | VarInput | Serial port path (e.g., `'/dev/ttyUSB0'`, `'/dev/ttyACM0'`) |
| `baud` | INT | VarInput | Baud rate: 9600, 19200, 38400, 57600, 115200 |
| `parity` | STRING | VarInput | Parity mode: `'N'` (none), `'E'` (even), `'O'` (odd) |
| `data_bits` | INT | VarInput | Data bits: `7` or `8` (default 8) |
| `stop_bits` | INT | VarInput | Stop bits: `1` or `2` (default 1) |
| `connected` | BOOL | Var | TRUE if the serial port is open and ready |
| `error_code` | INT | Var | 0 = OK (see SerialLink error codes below) |

#### SerialLink error codes

| Code | Meaning |
|------|---------|
| 0 | OK — port is open |
| 1 | No port configured (port string is empty) |
| 2 | Port open failed (device doesn't exist, permission denied, or busy) |
| 3 | Port lost (was open, now disconnected) |

### Modbus RTU device error codes

| Code | Meaning |
|------|---------|
| 0 | OK — device is responding |
| 1 | No link configured (link string is empty) |
| 2 | No slave configured (slave_id is 0) |
| 10 | Communication error (timeout, CRC mismatch, or device not responding) |

### Non-blocking I/O

Device I/O runs on a **background thread** (one per serial port), not on the
PLC scan cycle. The `execute()` method only copies cached values:

- **Read path**: background thread reads registers at `refresh_rate` intervals,
  stores results in a shared buffer. `execute()` copies the latest values into
  the FB's field slots.
- **Write path**: `execute()` writes the latest output values to a shared buffer.
  The background thread picks up the latest values on its next I/O cycle
  (last-value-wins, no queue).
- **Batching**: consecutive registers of the same kind are read/written in a
  single Modbus transaction to minimize bus traffic.
- **Bus coordination**: all devices sharing a serial port are polled by the same
  thread in round-robin order, preventing half-duplex bus contention.

### Simulated device layout

For `simulated` profiles, the generated FB has a simpler layout (no link required):

1. `refresh_rate : TIME` (VAR_INPUT)
2. `connected : BOOL`, `error_code : INT`, `io_cycles : UDINT`, `last_response_ms : REAL` (VAR)
3. I/O fields from the profile (VAR)

## Complete example: Waveshare 8-channel analog input

```yaml
name: WaveshareAnalogInput
vendor: Waveshare
protocol: modbus-rtu
description: "Waveshare Analog Input 8CH"

fields:
  - { name: AI1, type: INT, direction: input, register: { address: 0, kind: input_register } }
  - { name: AI2, type: INT, direction: input, register: { address: 1, kind: input_register } }
  - { name: AI3, type: INT, direction: input, register: { address: 2, kind: input_register } }
  - { name: AI4, type: INT, direction: input, register: { address: 3, kind: input_register } }
  - { name: AI5, type: INT, direction: input, register: { address: 4, kind: input_register } }
  - { name: AI6, type: INT, direction: input, register: { address: 5, kind: input_register } }
  - { name: AI7, type: INT, direction: input, register: { address: 6, kind: input_register } }
  - { name: AI8, type: INT, direction: input, register: { address: 7, kind: input_register } }
```

Usage in ST:

```st
PROGRAM Main
VAR
    serial : SerialLink;
    adc    : WaveshareAnalogInput;
END_VAR
    serial(port := '/dev/ttyACM0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);

    adc(
        link := serial.port,
        slave_id := 1,
        refresh_rate := T#50ms
    );

    IF adc.connected THEN
        (* Read analog channels *)
        SupplyVoltage := adc.AI1;
    END_IF;
END_PROGRAM
```

## Simulated devices

For testing without hardware, use `protocol: simulated`:

```yaml
name: SimIo
protocol: simulated
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: virtual } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 1, kind: virtual } }
```

Simulated devices get a web UI at `http://localhost:8080+` where you can
toggle inputs and watch outputs update in real time.
