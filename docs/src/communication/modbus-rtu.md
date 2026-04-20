# Modbus RTU

Modbus RTU is the most common serial protocol in industrial automation.
This guide covers everything you need to connect Modbus RTU devices to
the PLC runtime.

## Hardware setup

### What you need

1. **RS-485 adapter** — USB-to-RS-485 converter (e.g., FTDI, CH340-based)
   or a built-in UART on Raspberry Pi
2. **Wiring** — 2-wire (A/B) or 4-wire RS-485 bus
3. **Termination** — 120Ω resistor at each end of the bus for long runs
4. **Device** — any Modbus RTU slave (I/O module, VFD, sensor, etc.)

### Typical wiring

```
PLC (RS-485 adapter)           Modbus Slave
  A (D+) ──────────────────── A (D+)
  B (D-) ──────────────────── B (D-)
  GND    ──────────────────── GND (optional but recommended)
```

For multiple devices on the same bus:

```
PLC ──── A ──┬── Device 1 (addr=1)
             ├── Device 2 (addr=2)
             └── Device 3 (addr=3)
         B ──┘ (same for B line)
```

### Raspberry Pi

The Raspberry Pi has a built-in UART at `/dev/ttyAMA0` (GPIO 14/15).
For RS-485, use a HAT or a USB adapter:

| Method | Port path | Notes |
|--------|-----------|-------|
| USB RS-485 adapter | `/dev/ttyUSB0` | Most common, plug-and-play |
| Raspberry Pi UART HAT | `/dev/ttyAMA0` | Requires disable of Bluetooth on Pi 3/4 |
| GPIO + MAX485 | `/dev/ttyAMA0` | Needs DE/RE pin control |

### Serial settings

Most Modbus RTU devices use these defaults:

| Parameter | Common values | Default |
|-----------|--------------|---------|
| Baud rate | 9600, 19200, 38400, 115200 | 9600 |
| Parity | None (N), Even (E), Odd (O) | Even (E) for Modbus standard |
| Data bits | 8 | 8 |
| Stop bits | 1 or 2 | 1 (with parity) or 2 (without parity) |

> **Note**: The official Modbus standard specifies 8E1 (8 data bits, even parity,
> 1 stop bit). However, many devices default to 8N1 or 8N2. Check your device's
> documentation.

## Step-by-step setup

### 1. Create the device profile

Create a YAML file in `profiles/` that maps your device's Modbus registers.
You'll need the device's register map from its manual.

**Example: 4-channel analog input module**

```yaml
# profiles/analog_4ch.yaml
name: Analog4ch
vendor: Generic
protocol: modbus-rtu
description: "4-channel analog input module (0-10V, 12-bit)"

fields:
  - name: CH_0
    type: INT
    direction: input
    register:
      address: 0
      kind: input_register
      scale: 0.00244     # 10V / 4096 counts = 0.00244 V/count
      unit: V

  - name: CH_1
    type: INT
    direction: input
    register:
      address: 1
      kind: input_register
      scale: 0.00244
      unit: V

  - name: CH_2
    type: INT
    direction: input
    register:
      address: 2
      kind: input_register
      scale: 0.00244
      unit: V

  - name: CH_3
    type: INT
    direction: input
    register:
      address: 3
      kind: input_register
      scale: 0.00244
      unit: V
```

### 2. Write the ST program

```st
PROGRAM Main
VAR
    serial   : SerialLink;
    analog   : Analog4ch;
    voltage  : REAL;
    alarm    : BOOL := FALSE;
END_VAR
    (* Configure and open the serial port *)
    serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'E',
           data_bits := 8, stop_bits := 1);

    (* Read analog inputs every 100ms *)
    analog(link := serial.port, slave_id := 1, refresh_rate := T#100ms);

    (* Process the analog values *)
    voltage := INT_TO_REAL(IN1 := analog.CH_0);

    (* High voltage alarm *)
    alarm := voltage > 8.0;

    (* Check communication status *)
    IF NOT analog.connected THEN
        (* Handle communication failure *)
    END_IF;
END_PROGRAM
```

### 3. Configure the project

```yaml
# plc-project.yaml
name: MyProject
version: "1.0.0"
entryPoint: Main
engine:
  cycle_time: 10ms
```

### 4. Run

```bash
cargo run -p st-cli -- run . -n 0    # run continuously
```

## Common device profiles

### VFD (Variable Frequency Drive)

VFDs typically expose speed reference, status, and fault information
via Modbus holding and input registers.

```yaml
name: GenericVfd
protocol: modbus-rtu
description: "Generic VFD with speed control"

fields:
  # Control outputs (PLC → VFD)
  - name: RUN
    type: BOOL
    direction: output
    register: { address: 0, kind: coil }

  - name: SPEED_REF
    type: REAL
    direction: output
    register:
      address: 0
      kind: holding_register
      scale: 0.1
      unit: Hz

  # Status inputs (VFD → PLC)
  - name: READY
    type: BOOL
    direction: input
    register: { address: 0, kind: discrete_input }

  - name: RUNNING
    type: BOOL
    direction: input
    register: { address: 1, kind: discrete_input }

  - name: FAULT
    type: BOOL
    direction: input
    register: { address: 2, kind: discrete_input }

  - name: SPEED_ACT
    type: REAL
    direction: input
    register:
      address: 0
      kind: input_register
      scale: 0.1
      unit: Hz

  - name: CURRENT
    type: REAL
    direction: input
    register:
      address: 1
      kind: input_register
      scale: 0.01
      unit: A
```

Usage:

```st
PROGRAM Main
VAR
    serial : SerialLink;
    vfd    : GenericVfd;
END_VAR
    serial(port := '/dev/ttyUSB0', baud := 19200, parity := 'E',
           data_bits := 8, stop_bits := 1);
    vfd(link := serial.port, slave_id := 2, refresh_rate := T#100ms);

    (* Start the drive at 30 Hz *)
    IF vfd.READY AND NOT vfd.FAULT THEN
        vfd.RUN := TRUE;
        vfd.SPEED_REF := 30.0;
    END_IF;

    (* Emergency stop *)
    IF emergency_stop THEN
        vfd.RUN := FALSE;
        vfd.SPEED_REF := 0.0;
    END_IF;
END_PROGRAM
```

### Digital I/O module

```yaml
name: DigitalIO16
protocol: modbus-rtu
description: "16-point digital I/O (8 DI + 8 DO)"

fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: discrete_input } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: discrete_input } }
  - { name: DI_2, type: BOOL, direction: input, register: { address: 2, kind: discrete_input } }
  - { name: DI_3, type: BOOL, direction: input, register: { address: 3, kind: discrete_input } }
  - { name: DI_4, type: BOOL, direction: input, register: { address: 4, kind: discrete_input } }
  - { name: DI_5, type: BOOL, direction: input, register: { address: 5, kind: discrete_input } }
  - { name: DI_6, type: BOOL, direction: input, register: { address: 6, kind: discrete_input } }
  - { name: DI_7, type: BOOL, direction: input, register: { address: 7, kind: discrete_input } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, kind: coil } }
  - { name: DO_1, type: BOOL, direction: output, register: { address: 1, kind: coil } }
  - { name: DO_2, type: BOOL, direction: output, register: { address: 2, kind: coil } }
  - { name: DO_3, type: BOOL, direction: output, register: { address: 3, kind: coil } }
  - { name: DO_4, type: BOOL, direction: output, register: { address: 4, kind: coil } }
  - { name: DO_5, type: BOOL, direction: output, register: { address: 5, kind: coil } }
  - { name: DO_6, type: BOOL, direction: output, register: { address: 6, kind: coil } }
  - { name: DO_7, type: BOOL, direction: output, register: { address: 7, kind: coil } }
```

### Temperature sensor (PT100)

```yaml
name: PT100_4ch
protocol: modbus-rtu
description: "4-channel PT100 RTD temperature module"

fields:
  - name: TEMP_0
    type: REAL
    direction: input
    register:
      address: 0
      kind: input_register
      scale: 0.1
      unit: "°C"

  - name: TEMP_1
    type: REAL
    direction: input
    register:
      address: 1
      kind: input_register
      scale: 0.1
      unit: "°C"

  - name: TEMP_2
    type: REAL
    direction: input
    register:
      address: 2
      kind: input_register
      scale: 0.1
      unit: "°C"

  - name: TEMP_3
    type: REAL
    direction: input
    register:
      address: 3
      kind: input_register
      scale: 0.1
      unit: "°C"
```

## Multiple devices on one bus

Multiple Modbus slaves can share a single RS-485 bus. Each device gets a
unique slave address (1-247).

```st
PROGRAM Main
VAR
    serial    : SerialLink;
    io_rack   : DigitalIO16;
    vfd       : GenericVfd;
    temp      : PT100_4ch;
END_VAR
    (* All devices share the same serial port *)
    serial(port := '/dev/ttyUSB0', baud := 19200, parity := 'E',
           data_bits := 8, stop_bits := 1);

    (* Each device has a unique slave_id *)
    io_rack(link := serial.port, slave_id := 1, refresh_rate := T#50ms);
    vfd(link := serial.port, slave_id := 2, refresh_rate := T#100ms);
    temp(link := serial.port, slave_id := 3, refresh_rate := T#500ms);

    (* Use different refresh rates based on priority:
       - I/O rack: fast (50ms) for responsive digital control
       - VFD: medium (100ms) for motor control
       - Temperature: slow (500ms) for monitoring *)
END_PROGRAM
```

## Troubleshooting

### Device not responding (`connected = FALSE`)

1. **Check wiring**: A/B lines may be swapped. Try swapping A and B.
2. **Check slave address**: Verify the device's DIP switches or configuration
   match the `slave_id` in your program.
3. **Check baud rate and parity**: Both sides must match exactly.
4. **Check termination**: For bus runs > 10m, add 120Ω termination.
5. **Check `error_code`**: Non-zero values indicate specific errors.

### Error codes

| Code | Meaning | Action |
|------|---------|--------|
| 0 | OK | — |
| 1 | No slave configured | Set `slave_id` parameter |
| 10 | Communication error | Check wiring, baud rate, slave address |
| 101 | Illegal function | Device doesn't support this register type |
| 102 | Illegal data address | Register address out of range |
| 103 | Illegal data value | Value out of range for the device |
| 104 | Slave device failure | Internal device error |

### Timing issues

If you see intermittent communication failures:

- **Increase `refresh_rate`**: Slow devices may not handle fast polling.
  Start with `T#500ms` and decrease until stable.
- **Reduce bus speed**: Try 9600 baud if 19200 is unreliable.
- **Check bus length**: RS-485 supports up to 1200m, but noise increases
  with distance. Use shielded twisted pair for long runs.

### Finding register addresses

Every Modbus device has a register map in its documentation. Look for:

- **Coils** (FC01/FC05): digital outputs, typically address 0xxxx
- **Discrete inputs** (FC02): digital inputs, typically address 1xxxx
- **Input registers** (FC04): analog inputs/measurements, address 3xxxx
- **Holding registers** (FC03/FC06): analog outputs/config, address 4xxxx

> **Important**: Some manufacturers use 1-based addressing in their docs
> but the Modbus protocol uses 0-based. If the manual says "register 40001",
> the actual Modbus address is 0. Subtract the prefix (40001 → 0).

## Running tests without hardware

Use `socat` to create virtual serial port pairs for testing:

```bash
# Terminal 1: create virtual serial pair
nix-shell -p socat --run "socat pty,raw,echo=0,link=/tmp/vpty0 pty,raw,echo=0,link=/tmp/vpty1"

# Terminal 2: run your program using the virtual port
cargo run -p st-cli -- run . -n 100
```

The integration test suite uses this approach with a built-in Modbus slave
simulator to verify all function codes work correctly.
