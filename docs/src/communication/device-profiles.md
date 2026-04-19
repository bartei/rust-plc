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
name: MyIoModule                    # Type name in ST code
vendor: ACME Corp                   # Optional
protocol: modbus-rtu                # Protocol: modbus-rtu, simulated
description: "16-channel I/O"       # Optional

fields:
  - name: DI_0                      # Field name in ST code
    type: BOOL                      # IEC 61131-3 data type
    direction: input                # input, output, or inout
    register:
      address: 0                    # Modbus register address
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
| `USINT` | 8-bit unsigned | 0..255 | Status bytes |
| `UINT` | 16-bit unsigned | 0..65535 | Analog I/O (raw) |
| `UDINT` | 32-bit unsigned | 0..2^32-1 | Timers, large counters |
| `REAL` | 32-bit float | ±3.4e38 | Temperature, speed, etc. |
| `LREAL` | 64-bit float | ±1.8e308 | High-precision measurements |

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

## Generated FB layout

When the runtime loads a profile, it creates a function block type with:

1. **Configuration parameters** (VAR_INPUT):
   - `link : INT` — serial link handle (for Modbus RTU)
   - `slave_id : INT` — Modbus slave address (1-247)
   - `refresh_rate : TIME` — how often to poll the device

2. **Diagnostic fields** (VAR):
   - `connected : BOOL` — device responding?
   - `error_code : INT` — 0 = OK
   - `io_cycles : UDINT` — successful I/O count
   - `last_response_ms : REAL` — last round-trip time

3. **I/O fields** (VAR) — one per field in the profile, in declaration order

## Complete example: WAGO 750-352 I/O coupler

```yaml
name: Wago750352
vendor: WAGO
protocol: modbus-rtu
description: "WAGO 750-352 Modbus coupler with 8 DI + 8 DO"

fields:
  # Digital inputs (FC02 discrete inputs)
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: discrete_input } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: discrete_input } }
  - { name: DI_2, type: BOOL, direction: input, register: { address: 2, kind: discrete_input } }
  - { name: DI_3, type: BOOL, direction: input, register: { address: 3, kind: discrete_input } }
  - { name: DI_4, type: BOOL, direction: input, register: { address: 4, kind: discrete_input } }
  - { name: DI_5, type: BOOL, direction: input, register: { address: 5, kind: discrete_input } }
  - { name: DI_6, type: BOOL, direction: input, register: { address: 6, kind: discrete_input } }
  - { name: DI_7, type: BOOL, direction: input, register: { address: 7, kind: discrete_input } }

  # Digital outputs (FC01/FC05 coils)
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, kind: coil } }
  - { name: DO_1, type: BOOL, direction: output, register: { address: 1, kind: coil } }
  - { name: DO_2, type: BOOL, direction: output, register: { address: 2, kind: coil } }
  - { name: DO_3, type: BOOL, direction: output, register: { address: 3, kind: coil } }
  - { name: DO_4, type: BOOL, direction: output, register: { address: 4, kind: coil } }
  - { name: DO_5, type: BOOL, direction: output, register: { address: 5, kind: coil } }
  - { name: DO_6, type: BOOL, direction: output, register: { address: 6, kind: coil } }
  - { name: DO_7, type: BOOL, direction: output, register: { address: 7, kind: coil } }
```

Usage in ST:

```st
PROGRAM Main
VAR
    serial : SerialLink;
    wago   : Wago750352;
END_VAR
    serial(port := '/dev/ttyUSB0', baud := 19200, parity := 'E',
           data_bits := 8, stop_bits := 1);
    wago(link := serial, slave_id := 1, refresh_rate := T#50ms);

    (* Mirror inputs to outputs *)
    wago.DO_0 := wago.DI_0;
    wago.DO_1 := wago.DI_1;

    (* Alarm on DI_7 *)
    IF wago.DI_7 THEN
        wago.DO_7 := TRUE;  (* alarm light *)
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
