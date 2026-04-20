# Device Communication

The PLC runtime communicates with external devices (I/O racks, VFDs, sensors)
through **native function blocks**. Device types are derived from YAML profiles
and appear as normal function blocks in your ST code — with full IDE support
(code completion, type checking, debugger variable expansion).

## How it works

1. You create a **device profile** (YAML file) that describes the device's
   registers — field names, data types, directions, and Modbus addresses.

2. The runtime discovers profiles in your project's `profiles/` directory and
   makes them available as function block types.

3. In your ST program, you declare instances of these types and call them
   each scan cycle. The runtime handles the actual serial communication.

## Quick example

```st
PROGRAM Main
VAR
    serial   : SerialLink;
    io_rack  : MyIoModule;
END_VAR
    (* Open the serial port — runs once on first call *)
    serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'N',
           data_bits := 8, stop_bits := 1);

    (* Read/write I/O every 50ms *)
    io_rack(link := serial.port, slave_id := 1, refresh_rate := T#50ms);

    (* Use the I/O fields *)
    io_rack.DO_0 := io_rack.DI_0;
    io_rack.AO_0 := io_rack.AI_0 * 2;
END_PROGRAM
```

## Supported protocols

| Protocol | Status | Link type | Crate |
|----------|--------|-----------|-------|
| **Simulated** | Working | None (in-memory) | `st-comm-sim` |
| **Modbus RTU** | Working | RS-485/RS-232 serial | `st-comm-modbus` |
| Modbus TCP | Planned | TCP/IP | — |
| EtherNet/IP | Planned | TCP/IP | — |

## Architecture

```
┌──────────────────┐     ┌────────────────────┐
│  ST Program       │     │  Device Profile     │
│                   │     │  (YAML)             │
│  serial(...)      │     │  - register map     │
│  io_rack(...)     │     │  - field types      │
│  io_rack.DO_0 :=  │     │  - scaling/offset   │
└────────┬──────────┘     └─────────┬───────────┘
         │                          │
         ▼                          ▼
┌──────────────────────────────────────────────┐
│  NativeFb Registry                           │
│  (built from profiles at startup)            │
│                                              │
│  SerialLink → opens /dev/ttyUSB0             │
│  MyIoModule → Modbus RTU FC01-FC10           │
└──────────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────────┐
│  Serial Transport (RS-485 bus)               │
│  - inter-frame timing                        │
│  - bus access mutex (multiple devices share)  │
│  - CRC16 validation                          │
└──────────────────────────────────────────────┘
```

## Diagnostic fields

Every device FB includes diagnostic fields that update automatically:

| Field | Type | Description |
|-------|------|-------------|
| `connected` | BOOL | TRUE if the device is responding |
| `error_code` | INT | 0 = OK, non-zero = error |
| `io_cycles` | UDINT | Number of successful I/O cycles |
| `last_response_ms` | REAL | Last round-trip time in milliseconds |

```st
IF NOT io_rack.connected THEN
    (* Handle communication failure *)
    alarm := TRUE;
END_IF;
```

## Next steps

- [Device Profiles](device-profiles.md) — how to create a YAML profile for your hardware
- [Modbus RTU](modbus-rtu.md) — detailed Modbus RTU setup and wiring guide
