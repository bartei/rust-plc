# Impac IGAR 6 Smart — UPP Protocol — Design Document

> **Progress tracker:** [implementation_igar.md](implementation_igar.md) — checklist and status.
> **Parent plan:** [implementation_core.md](implementation_core.md) — core platform progress tracker.
> **Sibling docs:**
> - [design_comm.md](design_comm.md) — communication-layer architecture (transport, native FBs, profiles)
> - [implementation_comm.md](implementation_comm.md) — communication-layer progress tracker

This document captures the **why** behind the UPP protocol crate. The
**what** and **status** live in `implementation_igar.md`.

---

## Why a new crate (and not an extension of `st-comm-modbus`)

UPP shares only one thing with Modbus: the underlying RS485 wire. Above
that, the two protocols are structurally different:

| Aspect | Modbus RTU | UPP |
|---|---|---|
| Frame encoding | Binary, byte-oriented | ASCII printable + `CR` terminator |
| Addressing | Slave ID byte + function code | 2-digit ASCII address + 2 lowercase letters |
| Error detection | CRC-16 (Modbus polynomial) | None (relies on parity + timeout-and-retry) |
| Multi-register access | Function codes 03/04/06/16 with explicit count | One command per parameter |
| Master timing | T1.5 inter-character / T3.5 inter-frame | "Master must wait ≥1.5 ms after response" |
| Response window | Implementation-defined (typ. 100 ms) | "Response within 5 ms at the latest" |
| Broadcast | Unit ID 0 | Address 99 (no response) |

Forcing UPP into the Modbus RTU client would either bloat that crate
with mode switches or require an awkward "encoder strategy" abstraction.
A separate crate keeps both protocols simple and independently testable.

What IS shared comes from `st-comm-serial` (`SerialTransport`,
`BusManager`, `BusDeviceIo`, `FrameParser`) and `st-comm-api`
(`NativeFb`, `NativeFbLayout`, `DeviceProfile`). The new crate plugs
into those, exactly the way `st-comm-modbus` does.

---

## Wire protocol — UPP (Universal Pyrometer Protocol)

### Serial line settings

Fixed by the device: **8 data bits, even parity, 1 stop bit (8E1), no
flow control**. Baud rate is configurable on the device itself
(`AAbrX` command); supported values are 1200, 2400, 4800, 9600, 19200,
38400, 57600, 115200. Factory default is **19200 baud**. The IGAR's
own `AAbr` command set means the host doesn't autonegotiate — the
project YAML must declare the baud the device is currently configured
for.

### Frame format

All traffic is printable ASCII terminated by `CR` (0x0D, ASCII 13).
There is no length field, no checksum, no STX/ETX — just a varying-length
ASCII payload followed by `CR`.

```
Request:    AA cc [param]   CR
Response:   payload          CR
            ^                ^
            same charset as
            the command's
            "Answer" column
```

- `AA` = 2-digit decimal address (`00`..`99`, see addressing below)
- `cc` = 2 lowercase ASCII letters identifying the command (`em`,
  `gt`, `ms`, `tr`, …)
- `param` = optional command-specific argument:
  - empty for read commands (e.g. `00em` = "read emissivity of device 00")
  - 4 ASCII decimal digits for typical writes (e.g. `00em0853` = "set
    emissivity of device 00 to 0.853")
  - Variable widths for some commands (`AAm1XXXXYYYY`, `AAfhX`,
    `AApa` returns 15 digits, etc.)
  - `?` for "read limits" (e.g. `00em?` returns the min/max for `em`)

The note "the letter 'l' means the lower-case letter of 'L'" in the
manual is a typography aid for printed manuals; it doesn't alter the
encoding. We treat it as a documentation-only nit.

### Response shape

The manual classifies command responses as one of:

| Class | Trigger | Response |
|---|---|---|
| Read with output | `AAcc` (no parameter), e.g. `00em` | `<payload>CR` (e.g. `0970CR`) |
| Write that echoes | `AAccXXXX`, e.g. `00em0853` | `<echoed value>CR` (e.g. `0853CR` per "Example Write" in §7) |
| Pure write | "set" commands listed without echo | `okCR` or `noCR` |
| Read limits | `AAcc?` | `<min><max>CR` (e.g. `00501000CR`) |

We model all four classes with one parser: read until `CR`, return the
ASCII payload. Decision logic on whether the payload is a value, an
echo, an `ok`/`no` ack, or a limits pair lives in the per-command
decoder.

### No checksum — error detection model

Error detection relies entirely on:
1. **8E1 parity** at the UART level. A character with bad parity is
   silently dropped by the UART; the resulting frame won't terminate
   in `CR` within the 5-ms window.
2. **5 ms response timeout**. If the master doesn't see `CR` within
   5 ms after sending the request, it must treat the transaction as
   failed and retry.
3. **1.5 ms post-response cooldown**. After receiving the `CR`, the
   master must wait ≥1.5 ms before sending the next request — older
   RS485 transceivers need that long to fully release the bus.

Compare to Modbus RTU's CRC + T1.5/T3.5: UPP is much weaker, but the
short response window and small frames keep collision risk low. Our
implementation MUST honor the 5 ms timeout exactly; otherwise we'll
mis-frame "no response" as "garbled response" and retry the wrong
thing.

### Addressing rules

| Address | Meaning |
|---|---|
| `00`..`97` | Individual device address. Set per device via `AAga` command. |
| `98` | Global broadcast **with** response. Only ONE device may be on the bus when using `98` (otherwise responses collide). Used to discover an unknown device. |
| `99` | Global broadcast **without** response. Used to push parameter writes to all devices simultaneously. |

### Half-duplex bus timing

From the manual's "Additional instruction for the RS485 interface":

1. After sending a request, the master must turn the bus around within
   **3 character times** (~1.5 ms at 19200 baud). Older USB-to-RS485
   adapters that auto-direction-switch may be too slow; the manual
   recommends LumaSense's own adapter (order no. 3 826 750).
2. Pyrometer responds within **5 ms** at the latest.
3. No `CR` within 5 ms ⇒ master assumes parity or syntax error and
   retries.
4. After receiving the response, master waits **≥1.5 ms** before the
   next command.

Our `SerialTransport` already handles half-duplex turnaround via
`clear_input_buffer()` after a write, so we only need to encode
timeouts (1) and (2)–(4) at the protocol layer.

---

## Crate architecture

### `st-comm-upp` layout

```
crates/st-comm-upp/
├── Cargo.toml                 (depends on st-comm-api, st-comm-serial, st-ir)
├── src/
│   ├── lib.rs                 (public re-exports)
│   ├── address.rs             (Address enum: Individual(0..=97), BroadcastWithResponse=98, BroadcastNoResponse=99)
│   ├── command.rs             (Command enum: Em, Et, Ev, Ez, Fh, Ga, Gt, Ka, La, Lz, Mb, Me, Ms, Na, Pa, Sn, Tr, Br, …; encode_request(&self) -> Vec<u8>)
│   ├── parser.rs              (Decoder trait, parse_response(bytes, expected_kind) -> Result<Response, UppError>)
│   ├── client.rs              (UppClient: holds SerialTransport ref, transaction_with_timeout(addr, cmd) with 5ms read deadline + 1.5ms cooldown)
│   ├── frame.rs               (Frame builder: format!("{:02}{}{}\r", addr, cmd_letters, params))
│   ├── frame_parser.rs        (FrameParser impl: scan for CR with hard 5ms deadline)
│   ├── device_fb.rs           (UppDeviceNativeFb: NativeFb impl; UppDeviceIo: BusDeviceIo impl for BusManager; IoState)
│   └── error.rs               (UppError: Timeout, Parity, BadResponse, OutOfRange, …)
└── tests/
    ├── upp_integration_test.rs   (socat + slave simulator, single-device scan)
    ├── upp_multi_device_test.rs  (socat + multi-slave on same RS485 bus, broadcast 99)
    └── full_stack_test.rs        (ST program → engine → SerialLink → UPP → simulator → field readback)
```

### Layout of `UppDeviceNativeFb`

Mirrors `ModbusRtuDeviceNativeFb`:

| Slot | Direction | Name | Type | Purpose |
|---|---|---|---|---|
| 0 | INPUT | `link` | STRING | Name of the SerialLink FB instance |
| 1 | INPUT | `device_id` | INT | UPP address (0..99) |
| 2 | INPUT | `refresh_rate` | TIME | Minimum interval between scans of this device |
| 3 | INPUT | `timeout` | TIME | Per-request response timeout; default 5 ms |
| 4 | INPUT | `cooldown` | TIME | Post-response gap; default 2 ms (manual says "≥1.5 ms") |
| 5 | VAR | `connected` | BOOL | True after first successful exchange |
| 6 | VAR | `error_code` | INT | Last UppError variant (0 = ok) |
| 7 | VAR | `errors_count` | UDINT | Total transaction failures since FB init |
| 8 | VAR | `io_cycles` | UDINT | Total successful round-trips |
| 9 | VAR | `last_response_ms` | REAL | Round-trip time of the most recent transaction |
| 10+ | VAR | (profile fields) | per profile | One entry per `field` in the device profile YAML |

Differences from Modbus RTU:
- `device_id` vs `slave_id` — same role, different name to match the manual's "Device Address".
- `cooldown` is added because UPP's required post-response gap is
  longer relative to its short response window. Modbus RTU folds this
  into the framing-timer.
- No `preamble` field — UPP doesn't have a "warm up the bus" pattern.

### Profile YAML

```yaml
# profiles/impac_igar_6_smart.yaml
name: ImpacIgar6Smart
vendor: Impac (Advanced Energy / LumaSense)
protocol: upp
description: "IGAR 6 Smart digital pyrometer; 100..2550 °C; UPP via RS485"

# Each field maps to ONE UPP command. The runtime polls reads cyclically
# and applies writes when the program assigns to the field.
fields:
  - name: temperature
    type: REAL
    direction: input
    upp:
      command: ms          # 5 decimal digits, last is 1/10 °C
      decoder: temp_5d_tenth
  - name: ratio_temperature
    type: REAL
    direction: input
    upp:
      command: ek
      decoder: temp_pair_5d_tenth
      channel: ratio       # second 5-digit group
  - name: internal_temperature
    type: REAL
    direction: input
    upp:
      command: gt          # 3 decimal digits, °C or °F
      decoder: temp_3d_int
  - name: signal_strength
    type: REAL
    direction: input
    upp:
      command: tr          # 4 decimal digits, 0000..1500
      decoder: u16_dec
  - name: emissivity
    type: REAL
    direction: inout
    upp:
      command: em          # 4 decimal digits, 0050..1000 → 0.050..1.000
      decoder: u16_dec_milli
  - name: emissivity_ratio_k
    type: REAL
    direction: inout
    upp:
      command: ev          # 0800..1200 → 0.800..1.200
      decoder: u16_dec_milli
  - name: response_time
    type: INT
    direction: inout
    upp:
      command: ez          # X = 0..6 enum
      decoder: enum_response_time
  - name: operation_mode
    type: INT
    direction: inout
    upp:
      command: ka          # 0=metal, 1=mono, 2=ratio, 3=Smart
      decoder: enum_op_mode
  - name: laser
    type: BOOL
    direction: inout
    upp:
      command: la          # 0=off, 1=on
      decoder: bool_digit
```

The `upp:` sub-block is a new wrinkle compared to Modbus's `register:`
block. Schema-wise it's a new oneOf branch in
`device-profile.schema.json` keyed by the parent `protocol` value.

### Decoder catalog

A small set of named decoders covers every command in the manual.
Adding a device just means picking from this set; we never hand-roll
parsing per device.

| Decoder | Wire shape | Type | Conversion |
|---|---|---|---|
| `temp_5d_tenth` | `SSSSS` (5 dec) | REAL | `value/10.0` °C or °F |
| `temp_pair_5d_tenth` | `SSSSSQQQQQ` (10 dec) | REAL | First or second group / 10.0 (channel field) |
| `temp_3d_int` | `DDD` (3 dec) | REAL | Direct integer °C or °F |
| `u16_dec` | 4 dec | REAL | Direct integer (e.g. signal strength 0..1500) |
| `u16_dec_milli` | 4 dec | REAL | `value/1000.0` (e.g. emissivity 0.050..1.000) |
| `enum_response_time` | 1 dec | INT | 0=min, 1=0.01 s, 2=0.05 s, … 6=10 s |
| `enum_op_mode` | 1 dec | INT | 0=metal, 1=mono, 2=ratio, 3=Smart |
| `bool_digit` | 1 dec | BOOL | 0 → FALSE, 1 → TRUE |
| `hex_u32` | 6 hex | UDINT | Reference number, serial number |
| `range_pair` | `XXXXYYYY` 8 hex | (INT, INT) | Basic / sub range |

### Polling strategy

`UppDeviceIo::poll()` runs on the BusManager thread, same as Modbus
RTU:

1. **Drain pending writes**: for each profile field with `inout` or
   `output` direction whose value changed since the last poll, send
   the corresponding write command. UPP can only do one parameter at
   a time, so writes are sequenced.
2. **Issue one read pass**: for each `input` / `inout` field listed in
   the profile, send the read command and record the answer in
   `IoState::read_values`.
3. **Update diagnostics**: bump `io_cycles`, set `connected` after the
   first successful read, record `last_response_ms`, increment
   `errors_count` on any 5-ms timeout.

Per the manual's 1.5 ms post-response gap, we sleep `cooldown` between
sub-transactions in a single `poll()` call. With 8 fields at 19200 baud
that's ~50 ms total per scan — acceptable for industrial pyrometer
work where measurements update at 1 ms granularity inside the device
but outer reporting cadences are typically 50–200 ms.

`refresh_rate` on the FB sets the floor: even if `poll()` finishes
faster, the BusManager won't call us again until the interval
elapses. This is how multiple pyrometers on the same bus get fair
share of bandwidth.

---

## Simulator design

### Why a separate simulator (and not extend `st-comm-sim`)

`st-comm-sim` is a pure in-memory `NativeFb` — it bypasses the serial
stack entirely. That's fine for testing the **runtime's** I/O wiring,
but it can't catch bugs in:

- Framing (missing `CR`, parity issues, wrong character widths)
- Timing (5 ms response, 1.5 ms cooldown, baud-related delays)
- Bus arbitration (multiple devices sharing one RS485 link)
- Address handling (broadcast 98 vs 99 vs individual)

Those are precisely the failure modes the IGAR work needs to lock
down. So the simulator opens the **other end of a socat PTY pair** and
speaks the real UPP wire format, exactly mirroring the
`crates/st-comm-modbus/tests/rtu_integration_test.rs` slave-simulator
pattern.

### `IgarSimulator` — what it does

```rust
pub struct IgarSimulator {
    address: u8,                  // 0..=97
    state: Arc<Mutex<DeviceState>>,
    port_path: String,            // socat PTY end "B"
}

impl IgarSimulator {
    pub fn spawn(addr: u8, port_path: String) -> JoinHandle<()> { ... }
}

struct DeviceState {
    emissivity: u16,              // 0050..1000
    transmittance: u16,
    emissivity_ratio_k: u16,      // 0800..1200
    response_time: u8,            // 0..6
    op_mode: u8,                  // 0..3
    laser: bool,
    measuring_value_x10: i32,     // 1500.0 °C → 15000
    ratio_value_x10: i32,
    internal_temp: u16,           // 0..98 °C
    sub_range_lo: u16,
    sub_range_hi: u16,
    serial_number: u32,
    // … one field per UPP command we support
}
```

The simulator:
1. Opens the PTY end at the configured baud, 8E1.
2. Reads bytes until `CR`, validates the address matches its own (or
   the global 98/99).
3. Parses the 2-letter command, dispatches to a per-command handler.
4. Builds the ASCII response per the manual's "Answer" column,
   appends `CR`, writes it back.
5. Honors the 5 ms response window — for tests we can deliberately
   delay past 5 ms to assert the client's timeout / retry behaviour.
6. Honors broadcast: address 98 generates a response; 99 mutates state
   silently with no reply (and the test must NOT expect one).

The simulator is the **only** test code in the new crate that needs
to encode the manual's response shapes. The client decodes them; the
simulator encodes them; tests verify they round-trip.

---

## Testing strategy

### Layered tests, no mocks (per project policy)

Three layers, each with a clear scope:

**1. Unit-level frame encode/decode tests** (`crates/st-comm-upp/src/`)
- `command::tests` — every `Command` variant encodes to the exact byte
  string the manual specifies. Counter-test the manual examples
  verbatim: `00em` → `b"00em\r"`, `00em0853` → `b"00em0853\r"`,
  `00em?` → `b"00em?\r"`.
- `parser::tests` — every decoder parses every wire shape from the
  manual. Edge cases: minimum & maximum legal values, malformed input
  (no `CR`, non-digit, address mismatch).

These DO live in the source tree under `#[cfg(test)] mod tests`. They
are *not* the user-facing acceptance suite — they pin the wire spec
itself. The user's "no unit tests" rule applies to behaviour that can
be reached through real collaborators; here, the only thing being
tested IS the encoder/decoder and there is no other way to exercise
them than to call them. They are the protocol's specification in
executable form.

**2. socat + simulator integration** (`crates/st-comm-upp/tests/upp_integration_test.rs`)
- Spawn socat as a PTY pair (port_a ↔ port_b)
- Spawn `IgarSimulator` on port_b
- Open `SerialTransport` on port_a, build `UppClient`
- Round-trip every command class: read parameter, write parameter,
  read measurement, read limits, broadcast write
- Assert timing: response within 5 ms, cooldown observed before next
  request
- Assert bus arbitration: two `IgarSimulator`s with addresses 00 and
  01, alternating reads, no cross-talk

Mirrors `st-comm-modbus/tests/rtu_integration_test.rs` exactly.
`socat` discovery + `ST_REQUIRE_SOCAT=1` gating mirrors that file.

**3. Full-stack acceptance** (`crates/st-comm-upp/tests/full_stack_test.rs`)
- Compile a small ST program that declares an Impac IGAR 6 device,
  reads `temperature`, drives a control output
- Run on the in-process Engine with real `SerialLink` + `UppDevice`
  FBs talking to `IgarSimulator` over socat
- Assert: program reads the simulator's mutating temperature value,
  writes `emissivity` from the program, the simulator observes the
  change

This is the only test that proves the runtime can actually use the
new protocol from end-to-end.

### Why no Playwright / VS Code tests

The IGAR protocol surfaces no UI of its own. It plugs into the
existing Monitor panel via the standard FB-field display, which is
already covered by `editors/vscode/test/ui/monitor-panel.spec.js`.

### Deferred — explicit limits of socat-based testing

socat PTY simulates a perfect serial channel: no parity errors, no
line noise, no transceiver direction-switch latency. So our tests
**cannot** exercise:

- Real parity-error recovery (we'd need a hardware fault injector or
  a custom PTY driver)
- Real RS485 transceiver turnaround timing — socat fakes both ends
  with byte-perfect pipes
- Voltage / impedance / cable-length issues on a long bus

These remain field-validation concerns. We document them here so
future test additions don't try to chase them with synthetic fixtures.

---

## Open questions / decisions deferred to implementation

1. **Baud rate enforcement**: the device persists its baud across
   power cycles and is set with the `AAbrX` command. Should the
   `UppDeviceNativeFb` refuse to start if the project YAML's
   serial-link baud doesn't match the device's last-configured baud?
   Probably yes (saves a confused-silence debugging session), with a
   diagnostic log that recommends running a one-shot baud-rate command
   from the host. Decide during implementation.
2. **`mode: acyclic`** for IGAR: every read is fast (sub-50 ms for the
   full field set), so cyclic with a 100–200 ms refresh is the
   default. Acyclic-only support deferred until a use case appears.
3. **Multi-channel reads (`ek`)**: the `ek` command returns 1-channel
   AND ratio temperature in one round-trip. The profile decoder
   `temp_pair_5d_tenth` with a `channel: one|ratio` selector lets us
   bind both as separate fields, but the runtime reads the same
   command twice unless we add request coalescing. Coalescing is a
   stretch; ship without it and revisit if traces show it matters.
4. **Limits as constraints**: every parameter has a `?` query that
   returns its allowed range. We could automate "the program tried to
   write 0.005 to emissivity but the device says 0.050 is the floor"
   diagnostics by caching the limits at FB-init time. Nice-to-have,
   not blocking. Tracked as a stretch item in
   `implementation_igar.md`.
