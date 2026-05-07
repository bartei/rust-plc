# Impac IGAR 6 Smart — UPP Protocol — Progress Tracker

> **Design document:** [design_igar.md](design_igar.md) — protocol spec,
> crate architecture, simulator design, testing strategy.
> **Parent plan:** [implementation_core.md](implementation_core.md) —
> core platform progress tracker.
> **Sibling docs:** [implementation_comm.md](implementation_comm.md),
> [design_comm.md](design_comm.md) — communication-layer baseline.

This file is the actionable checklist. The **why** for each section
lives in `design_igar.md`; this file should stay terse.

> Workspace summary: 1598 Rust tests passing after Phase 9
> (+134 from baseline 1464 on master `a67ebe0`). The IGAR work
> ships the `st-comm-upp` crate, the multi-address IgarSimulator
> test helper, 11 simulator self-tests, 12 client↔simulator
> integration tests, and 1 full-stack E2E (ST → engine → simulator)
> — all socat-gated by `ST_REQUIRE_SOCAT=1`. Zero clippy warnings;
> no unit-mock fixtures.

---

## Phase 1 — Crate skeleton (DONE 2026-05-06)

- [x] Add `crates/st-comm-upp/` to the workspace `Cargo.toml`
      `members` array
- [x] Create `crates/st-comm-upp/Cargo.toml` with deps:
      `st-comm-api` (local), `st-comm-serial` (local), `st-ir`
      (workspace), `tracing`; `serialport = "4"` as a dev-dep for the
      future integration tests
- [x] `src/lib.rs` — public re-exports, crate-level docs
- [x] `src/error.rs` — `UppError` (Timeout, AddressMismatch,
      BadResponse, OutOfRange, NotImplemented, Transport) with
      stable `i16` diagnostic codes for the runtime FB layer
- [x] `cargo build -p st-comm-upp` succeeds
- [x] `cargo clippy -p st-comm-upp --tests -- -D warnings` clean

## Phase 2 — Frame layer (DONE 2026-05-06)

- [x] `src/address.rs` — `Address` enum (`Individual(0..=97)`,
      `BroadcastWithResponse=98`, `BroadcastNoResponse=99`);
      `encode()` 2-byte prefix; `parse()` round-trip;
      `expects_response()` so the client knows when to skip the read
- [x] `src/command.rs` — `Command` enum covering every UPP opcode
      from manual §7 (em/et/ev/dw/aw/ez/lz/fh/ka/la/as/lx/ga/mb/me/
      m1/m2/ms/ek/tm/gt/tr/sn/bn/na/pa/ve/vs/vc/br) plus a
      `ReadLimits(LimitsTarget)` for the `?` query form. Each
      variant has a range-checked `encode_request(addr)` →
      `b"AAcc[param]\r"`
- [x] Manual-example spec tests (executable specification):
  - [x] §7 "Read Command `00em` + CR" → exact bytes `b"00em\r"`
  - [x] §7 "Write `00emXXXX` + CR" with value 853 → `b"00em0853\r"`
  - [x] §7 "Read Limits `00em?` + CR" → `b"00em?\r"`
  - [x] Address-prefix padding (single-digit → `42em\r`)
  - [x] Broadcast 99 + write parameter (`b"99em0853\r"`)
  - [x] Range-rejection: ε < 50, ε > 1000, K out of 800..1200,
        switch-off < 2, baud-rate selector 7 (manual: "is not
        allowed")
  - [x] Sweep test: every Command variant has the right address
        prefix and CR terminator
- [x] `src/parser.rs` — `Decoder` enum: `Temp5dTenth`,
      `TempPair5dTenth { channel }`, `Temp3dInt`, `U16Dec4`,
      `U16DecMilli`, `Enum1`, `Enum1Wide`, `BoolDigit`, `HexPair8`,
      `Ack`, `Text`. Tests pin every "Answer" example from the
      manual:
  - [x] §7 "Answer 0970 means ε = 0.970"
  - [x] §7 "Answer 00em0853 means ε = 0.853"
  - [x] §7 "Answer 00501000 means ε ∈ 0.050..1.000" (limits)
  - [x] `ms` 5-digit /10 decode at low/mid/high end
  - [x] `ek` 10-digit pair, both channels
  - [x] `gt` internal-temp 3-digit
  - [x] `tr` signal-strength 4-digit at 0 / 1500
  - [x] `na` 16-char device-type string (with trailing spaces)
  - [x] Error paths: short / long / non-digit / bad ack / bad bool
- [x] `src/frame_parser.rs` — `UppFrameParser` implements
      `st_comm_serial::FrameParser`: scans for `CR`,
      `FrameStatus::Need(n+1)` → `FrameStatus::Complete(n+1)` the
      moment CR arrives, `FrameStatus::Invalid` if the buffer
      exceeds `MAX_RESPONSE_LEN = 64` (defence against a runaway
      device). The 5 ms wall-clock deadline is enforced by the
      transport's `timeout` parameter at call time, NOT by the
      parser — that's where it belongs per the
      `transaction_framed` contract.

**Test count:** 66 spec tests (address: 8, command: 19, parser: 22,
frame_parser: 7, error: 2). Workspace `cargo test` total:
1464 → 1530.

## Phase 3 — Client (DONE 2026-05-06)

- [x] `src/client.rs` — `UppClient` holding an
      `Arc<Mutex<SerialTransport>>`
- [x] `UppClient::new()` and `UppClient::with_timing(...)` — public
      constructors with the manual's defaults (5 ms timeout, 2 ms
      cooldown for safety margin over the spec minimum 1.5 ms,
      zero preamble)
- [x] `transaction(addr, cmd) -> Result<(UppResponse, TransactionStats), UppError>`:
      lock transport → flush input → send framed request →
      `transaction_framed` with `UppFrameParser` and the configured
      timeout → drop lock → cooldown
- [x] Broadcast 99 fast-path: send-only, skip the read entirely,
      return `UppResponse::NoResponse` (no guaranteed 5 ms wait)
- [x] Broadcast 98 acceptance: response carries the responding
      device's individual address, NOT 98 — `addresses_match()`
      special-cases this so individual reads still require an exact
      prefix match while broadcast-with-response accepts any
      individual address back
- [x] `transact()` typed convenience: runs a transaction and
      decodes the payload through a `Decoder`, with command-echo
      stripping for write replies (`00em0853` → `0853`) and limits
      queries (`em00501000` → `00501000`)
- [x] `From<String> for UppError` lifts the transport's
      `Result<_, String>` so `?` does the right thing — and the
      "Receive timeout" prefix from `transaction_framed` maps to
      `UppError::Timeout` (code 1) instead of being buried in
      `Transport` (code 6)
- [x] `sleep_at_least()` helper: OS sleep to `d - 100µs`, then
      spin-loop to the deadline. Honours the manual's 1.5 ms
      post-response cooldown on platforms whose timer resolution is
      coarser than 1 ms
- [x] `TransactionStats` captures wall-clock round-trip duration —
      ready to wire into the FB's `last_response_ms` field in
      Phase 4

**Test coverage** (15 new tests this phase, 81 total in the crate):
prefix strip, address-mismatch rejection, missing-CR rejection,
broadcast-98 individual-address-back acceptance, broadcast-98
broadcast-address-back rejection, write-echo strip for
`em`/`et`/`ev`/etc., limits-query echo strip, read pass-through,
ack pass-through (`ok`/`no`), full pipeline `00em` → 0.970 and
write-echo `00em0853` → 0.853, transport-timeout classification,
`sleep_at_least` doesn't undersleep / no-op on zero. Real-transport
round-trip tests (socat) are scheduled for Phase 8.

## Phase 4 — Native FB integration (DONE 2026-05-06)

- [x] `src/device_fb.rs` — `UppDeviceNativeFb` implementing
      `st_comm_api::NativeFb`. Layout per
      [design_igar.md](design_igar.md#layout-of-uppdevicenativefb):
      slots 0–4 INPUT (link, device_id, refresh_rate, timeout,
      cooldown), slots 5–9 diag, slot 10+ profile fields
- [x] `IoState` (Arc<Mutex<…>>): `read_values: Vec<Value>`,
      `write_values: Vec<Option<Value>>` keyed by field index
- [x] `UppDeviceIo` implementing `st_comm_serial::BusDeviceIo`:
      `poll(&transport)` runs one full read pass + queued writes,
      bumps diagnostics
- [x] `execute()` registers the device with `BusManager` on first
      cycle, then snapshots fields ↔ `IoState`
- [x] `to_upp_device_layout()` helper in
      `crates/st-comm-api/src/native_fb.rs` (mirrors
      `to_modbus_rtu_device_layout()`) — produces the fixed prefix +
      profile-driven suffix
- [x] `src/profile_binding.rs` — resolves YAML strings (`command`,
      `decoder`, `channel`) into typed `Command` + `Decoder` at FB
      construction time, keeping st-comm-api free of UPP-specific
      types
- [x] Diagnostic codes wired: `ERR_OK=0`, `ERR_NO_LINK=100`,
      `ERR_BAD_ADDRESS=101`, `ERR_PROFILE=102`; transport timeouts
      surface as the existing `UppError::Timeout` code (1)

## Phase 5 — Profile YAML + schema (DONE 2026-05-06)

- [x] Profile YAML schema: extended
      `schemas/device-profile.schema.json` with a per-field `upp:`
      object (`command`, `decoder`, optional `channel` /
      `param_width` / `count`); each field requires either
      `register` or `upp` via `oneOf`
- [x] Project YAML schema: added `"upp"` to
      `schemas/plc-project.schema.json` `device.protocol` enum
- [x] Profile YAML: shipped reference profile at
      `profiles/impac_igar_6_smart.yaml` (19 fields covering
      temperature, ratio, peak, internal, signal_strength,
      emissivity, transmittance, K, response_time, modes, laser,
      thresholds, device_type_text, serial_number_hex)
- [x] `DeviceProfile` parsing in `st-comm-api/src/profile.rs`:
      `register: Option<RegisterMapping>`, `upp: Option<UppFieldBinding>`
      with mutually-exclusive validation in profile_binding.rs
- [x] Acceptance regression: extended
      `editors/vscode/src/test/suite/schema.test.ts` with 5 new
      tests (UPP protocol, upp field binding, oneOf register/upp,
      project-yaml protocol, full IGAR profile shape) — 12 schema
      tests passing total
- [x] `crates/st-comm-upp/tests/profile_yaml.rs` — 3 integration
      tests load the shipped IGAR profile, resolve every field, and
      assert command/decoder bindings match manual §7

## Phase 6 — Runtime registration (DONE 2026-05-06)

- [x] Updated `crates/st-target-agent/src/api/program.rs`
      `build_native_fb_registry()` to dispatch on `profile.protocol`:
      `"simulated"` → `SimulatedNativeFb`, `"upp"` →
      `UppDeviceNativeFb` with shared `transport_map` +
      `BusManager`; auto-registers the matching `SerialLink` FB so
      profiles can reference it by name
- [x] `crates/st-cli/src/comm_setup.rs` mirror branch: `"upp"`
      protocol uses the same shared bus_manager / has_serial_protocol
      flag so CLI runs match agent runs
- [x] Cargo deps added: `st-comm-serial` + `st-comm-upp` for
      `st-target-agent`; `st-comm-upp` for `st-cli`
- [x] Smoke: `cargo build -p st-target-agent`,
      `cargo build -p st-cli`, full workspace clippy clean

## Phase 7 — Simulator (DONE 2026-05-06)

- [x] `crates/st-comm-upp/tests/igar_simulator.rs` (test-only
      helper module): `IgarSimulator::spawn(port, baud, addr)` opens
      a PTY end at 8E1, parses UPP requests, mutates internal state,
      encodes responses per manual §7's "Answer" column. Background
      thread driven by stop flag + JoinHandle
- [x] `IgarState::factory_defaults()` covers every command our
      `Decoder` enum reads: emissivity (1.000), transmittance,
      ratio K, response_time, op_mode, laser, measuring_value_x10,
      ratio temp, internal temp, basic/sub range, serial_number,
      device_type ("IGAR 6 Smart    " — 16 ASCII chars per manual)
- [x] Address handling: individual reads for own addr; ignore other
      addrs; honor `98` (respond using individual addr in reply) and
      `99` (apply, no respond)
- [x] Test-time fault injection knobs: `delay_response_ms`,
      `drop_next_response` — driven by the shared `Arc<Mutex<…>>`
      so tests can configure mid-run
- [x] `crates/st-comm-upp/tests/simulator_self_test.rs` — 11
      socat-based self-tests gated by `ST_REQUIRE_SOCAT=1`:
      read_emissivity, write_emissivity, limits_query, ms_5_digits,
      pair_10_digits, device_type_text, broadcast_99,
      broadcast_98, address_filtering, delay_response_ms,
      drop_next_response

**Workspace test count after Phase 7:** 1585 passing, 0 failing
(was 1464 pre-IGAR; +121 new tests across address/command/parser/
frame_parser/error/client/device_fb/profile_binding/profile_yaml/
simulator_self_test). Full `cargo clippy --workspace --tests
--all-features -- -D warnings` clean.

## Phase 8 — Integration tests (DONE 2026-05-07)

- [x] `crates/st-comm-upp/tests/upp_integration_test.rs` — socat
      PTY-pair spawn helper (copy from the modbus suite, gated by
      `ST_REQUIRE_SOCAT=1`), 12 integration tests
- [x] Per-command-class reads: `em` (parameter), `ms` (measurement),
      `em?` (limits), `na` (text)
- [x] Write-then-read round-trip: `00em0853` echo → state mutates →
      follow-up `00em` returns `0853`
- [x] Bus timing: 10 consecutive transactions take ≥ 18 ms (10 × 2 ms
      cooldown floor) — proves the post-response pause is honoured
- [x] Timeout path: `delay_response_ms = 50` → client returns
      `UppError::Timeout` (code 1), recoverable
- [x] Retry: `drop_next_response = true` → first call times out,
      second call on the same client succeeds
- [x] Multi-device topology — socat PTYs cannot be opened twice, so
      `IgarSimulator::spawn_multi(port, baud, &[a1, a2, ...])` hosts
      N virtual devices in one simulator process; covers two-address
      alternating reads (no cross-talk), broadcast 99 distribution
      (both devices apply, none respond), broadcast 98 single-device
      reply with individual-address echo, broadcast 98 collision
      classified cleanly without crashing the client

## Phase 9 — Full-stack E2E (DONE 2026-05-07)

- [x] `crates/st-comm-upp/tests/full_stack_test.rs` — compiles an
      ST program declaring `serial : SerialLink` + `pyro : IgarPyro`
      (UPP profile inlined as YAML literal so the test is
      self-contained), runs the in-process `Vm` with the registry
      holding both `SerialLinkNativeFb` and `UppDeviceNativeFb`
- [x] Asserts: `g_connected = TRUE`, `g_temperature = 1234.5`
      (from the simulator's seeded `measuring_value_x10 = 12345`),
      `g_errors = 0`
- [x] Round-trip write: ST program does `pyro.emissivity := 0.853`
      every cycle → simulator's `emissivity` field is 853 after a
      few cycles + sleep, proving the queued-write path through
      `BusManager` reaches the wire

## Phase 10 — Stretch / nice-to-haves (not blocking initial release)

- [ ] Limits caching: at FB-init, query `?` for every settable
      field; surface as constants on the FB instance for diagnostic
      use ("write rejected: out of [0050..1000]")
- [ ] `mode: acyclic` support — manual one-shot reads driven by an
      ST function call rather than the BusManager loop
- [ ] Request coalescing for the `ek` command (returns 1-channel +
      ratio in one round-trip)
- [ ] Auto-baud probe: if first transaction times out, retry at
      19200 (factory default) with a one-line warning; saves a
      common bring-up confusion
- [ ] InfraWin-compatible parameter dump (`pa` returns 15 packed
      digits) — surface as a single REAL[8] array field

## Verification gates (must pass before merge)

- [ ] `cargo clippy --workspace --tests --all-features -- -D warnings`
- [ ] `cargo test --workspace --exclude st-comm-modbus` — full Rust
      suite
- [ ] `ST_REQUIRE_SOCAT=1 cargo test -p st-comm-upp -p
      st-comm-serial -p st-comm-modbus -- --test-threads=1` — comm
      crates with socat
- [ ] Schema E2E: `npm --prefix editors/vscode run test:schema` (xvfb)
- [ ] No new dependabot vulnerabilities introduced (the new crate
      adds no transitive deps not already in the workspace)
- [ ] `plan/implementation_core.md` Cross-Cutting Concerns row
      "Testing: 1464+ tests" updated to reflect the new IGAR test
      count

## Deferred (rationale in design_igar.md)

- [ ] Real parity-error recovery — socat PTYs are byte-perfect; only
      reachable on hardware
- [ ] RS485 transceiver turnaround timing — socat fakes both ends
- [ ] Voltage / impedance / long-cable validation — field testing
      only

## Cross-cutting follow-ups (other docs)

- [ ] `plan/implementation_comm.md` — add a one-line link to this
      tracker under a new "Per-protocol crates" subsection
- [ ] `plan/design_comm.md` — note that UPP is the second protocol
      crate using the shared `SerialTransport` + `BusManager`, so the
      pattern is now general (not Modbus-specific)
- [ ] `docs/src/` — when ready, add a brief user-facing page for
      "Connecting an Impac IGAR 6 pyrometer" once Phase 9 lands
