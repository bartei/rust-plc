# Modbus RTU Manual Testing Checklist

## Prerequisites

- RS-485 USB adapter (e.g., FTDI, CH340) connected to `/dev/ttyUSB0`
- At least one Modbus RTU slave device on the bus
- Know the slave's address, baud rate, parity, and register map
- Or: use socat for virtual serial testing (no hardware needed)

---

## 1. IDE Integration

Open `playground/modbus_demo/` in your IDE.

- [x] `WaveshareAnalogInput` is recognized as a type — no red squiggles
- [x] `input.` triggers dot-completion showing: link, slave_id, refresh_rate, connected, error_code, io_cycles, last_response_ms, AI1..AI8
- [x] `SerialLink` is recognized as a type — no red squiggles on `serial : SerialLink`
- [x] `serial.` triggers dot-completion showing: port, baud, parity, data_bits, stop_bits, connected, error_code
- [x] Hover over `WaveshareAnalogInput` shows the FUNCTION_BLOCK signature
- [x] Hover over `input.AI1` shows `INT`
- [x] `st-cli check playground/modbus_demo` reports OK
- [x] Two-layer syntax: `serial(port := ...); input(link := serial.port, ...)` compiles clean

---

## 2. Virtual Serial Test (socat, no hardware)

### Setup

Terminal 1 — start virtual serial pair + slave simulator:
```bash
nix-shell -p socat --run "socat pty,raw,echo=0,link=/tmp/vpty0 pty,raw,echo=0,link=/tmp/vpty1"
```

Terminal 2 — run the automated test suite to verify everything works:
```bash
nix-shell -p socat pkg-config systemdLibs --run \
  "cargo test -p st-comm-modbus --test full_stack_test -- --nocapture"
```

- [x] Full-stack test passes (SerialLink + device with link := serial.port, connected=TRUE, DI_0=TRUE, AI_0=4200)

### Manual socat test

Edit `playground/modbus_demo/main.st` to use the virtual port:
```st
serial(port := '/tmp/vpty0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);
input(link := serial.port, slave_id := 1, refresh_rate := T#50ms);
```

Then run:
```bash
# Terminal 2: run the PLC program (5 cycles)
nix-shell -p socat pkg-config systemdLibs --run \
  "cargo run -p st-cli -- run playground/modbus_demo -n 5"
```

- [ ] Program compiles and starts without errors
- [ ] Reports cycle execution stats
- [ ] No panics or crashes

---

## 3. Real Hardware Test

### Setup

1. Connect RS-485 adapter to your PC
2. Wire to the Modbus slave (A/B/GND)
3. Note the slave address, baud rate, parity from the device manual
4. Create a profile for your device in `playground/modbus_demo/profiles/`

### Create your device profile

Example for a real device — replace with your actual register map:

```yaml
# playground/modbus_demo/profiles/my_real_device.yaml
name: MyRealDevice
protocol: modbus-rtu
description: "My actual Modbus device"

fields:
  # Adjust addresses to match your device's register map
  - name: INPUT_1
    type: INT
    direction: input
    register:
      address: 0          # Check your device manual
      kind: input_register # or holding_register, discrete_input, coil
      scale: 0.1           # if the device uses scaling
      unit: "°C"

  - name: OUTPUT_1
    type: INT
    direction: output
    register:
      address: 0
      kind: holding_register
```

### Update main.st

```st
PROGRAM Main
VAR
    serial : SerialLink;
    dev    : MyRealDevice;
END_VAR
    serial(
        port := '/dev/ttyUSB0',     (* adjust to your adapter *)
        baud := 9600,                (* match your device *)
        parity := 'E',              (* E for even, N for none, O for odd *)
        data_bits := 8,
        stop_bits := 1
    );

    dev(
        link := serial.port,
        slave_id := 1,              (* match your device's address *)
        refresh_rate := T#100ms
    );
END_PROGRAM
```

### Run and verify

```bash
cargo run -p st-cli -- run playground/modbus_demo -n 0
```

- [ ] `dev.connected` becomes TRUE (visible in debugger or monitor panel)
- [ ] `dev.error_code` is 0
- [ ] Input fields show real values from the device
- [ ] `dev.io_cycles` increments each refresh interval
- [ ] `dev.last_response_ms` shows reasonable round-trip time (typically 5-50ms)

### Write test

Add a write to the program:
```st
dev.OUTPUT_1 := 1234;
```

- [ ] The value appears on the real device (check with a Modbus tool or the device's display)
- [ ] Reading it back confirms the write

---

## 4. Multiple Devices on One Bus

Connect two Modbus slaves with different addresses. Create profiles for both.

```st
PROGRAM Main
VAR
    serial : SerialLink;
    dev1   : Device1;
    dev2   : Device2;
END_VAR
    serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'E',
           data_bits := 8, stop_bits := 1);
    dev1(link := serial.port, slave_id := 1, refresh_rate := T#100ms);
    dev2(link := serial.port, slave_id := 2, refresh_rate := T#100ms);
END_PROGRAM
```

- [x] Both devices show connected=TRUE
- [x] Both devices return correct independent values
- [x] No bus collisions (single bus thread per port, round-robin polling)

---

## 5. Error Handling

### Wrong slave address

Set `slave_id` to an address with no device:
```st
dev(... slave_id := 99, ...);
```

- [ ] `dev.connected` becomes FALSE after timeout
- [ ] `dev.error_code` is non-zero (10 = communication error)
- [ ] Program continues running (no crash)

### Wrong baud rate

Set baud to something the device doesn't support:
```st
dev(... baud := 115200, ...);  (* device expects 9600 *)
```

- [ ] `dev.connected` stays FALSE
- [ ] `dev.error_code` is non-zero

### Disconnected cable

While the program is running, unplug the RS-485 cable:

- [ ] `dev.connected` goes to FALSE within a few cycles
- [ ] `dev.error_code` shows an error
- [ ] Reconnecting the cable restores communication within a few cycles

### Wrong port path

```st
dev(port := '/dev/ttyNONEXISTENT', ...);
```

- [ ] `dev.connected` is FALSE
- [ ] `dev.error_code` is 3 (transport open failed)
- [ ] Program runs without crashing

---

## 6. Debugger Integration

Launch the debugger on the modbus_demo project:

- [ ] Variables panel shows `io` as expandable with all fields
- [ ] `io.connected`, `io.DI_0`, `io.AI_0` etc. update when stepping
- [ ] Can force `io.DO_0 := TRUE` via debug console
- [ ] Monitor panel shows Modbus device variables in watch list

---

## 7. Raspberry Pi Deployment (if available)

Deploy to a Raspberry Pi with an RS-485 adapter:

```bash
st-cli target install pi@raspberrypi
# then upload the modbus_demo project
```

- [ ] Program starts on the Pi
- [ ] Modbus communication works via `/dev/ttyUSB0` or `/dev/ttyAMA0`
- [ ] Variables visible via the remote monitor panel
- [ ] Survives reboot (auto-start with correct port config)

---

## Cleanup

After testing, you can delete this file:
```bash
rm plan/manual_modbus_testing.md
```
