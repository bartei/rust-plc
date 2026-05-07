//! `IgarSimulator` — a test-time UPP slave that speaks the wire.
//!
//! Built for use in Phase 8 integration tests:
//!
//! ```ignore
//! #[path = "igar_simulator.rs"]
//! mod igar_simulator;
//! use igar_simulator::{IgarSimulator, IgarState};
//! ```
//!
//! The simulator opens the *other* end of a socat-created PTY pair,
//! reads UPP requests until `CR`, dispatches per the manual's §7
//! command table, and writes back the manual's "Answer" shape. It
//! mirrors the Modbus RTU slave simulator in
//! `crates/st-comm-modbus/tests/rtu_integration_test.rs` — same
//! socat plumbing, ASCII frames instead of binary CRC ones.
//!
//! ## What it covers
//!
//! - Every [`Decoder`](st_comm_upp::Decoder) shape the
//!   `impac_igar_6_smart.yaml` profile reads (temperature, ratio,
//!   internal temp, signal strength, ε / τ / K, response-time enum,
//!   op-mode enum, °C/°F flag, laser bool, dirty-window, switch-off,
//!   device-type text, serial number).
//! - Address routing per manual §4.14: replies to own individual
//!   address, replies to broadcast 98 (with own-address echo),
//!   silently applies broadcast 99 writes.
//!
//! ## Fault injection (for negative-path tests)
//!
//! - [`IgarState::delay_response_ms`] — sleep before emitting the
//!   response. Tests use this to provoke the client's 5 ms
//!   response-window timeout.
//! - [`IgarState::drop_next_response`] — silently swallow the next
//!   response, then resume normally. Lets tests verify retry
//!   behaviour without crashing the simulator thread.
//!
//! ## Limitation
//!
//! socat PTYs are byte-perfect: there is no way to inject parity
//! errors at this layer. Real-hardware parity-error recovery
//! remains a field-test concern (documented in
//! `plan/design_igar.md` "Limits of socat-based testing").

#![allow(dead_code)] // helpers added incrementally as Phase 8 needs them

use serialport::{DataBits, Parity, SerialPort, StopBits};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Per-instance state — read by the request handler each cycle, may
/// be mutated by the test harness via the public methods on
/// [`IgarSimulator`].
#[derive(Debug, Clone)]
pub struct IgarState {
    /// 4-digit /1000 fields (manual: 0050..1000 ⇒ 0.050..1.000).
    pub emissivity: u16,
    pub transmittance: u16,
    /// Manual: 0800..1200 ⇒ 0.800..1.200.
    pub emissivity_ratio_k: u16,

    /// 1-digit selectors.
    pub response_time: u8,    // 0..=6
    pub clear_peak_mode: u8,  // 0..=9
    pub op_mode: u8,          // 0..=3
    pub fahrenheit: u8,       // 0 or 1
    pub laser: u8,            // 0 or 1
    pub analog_output: u8,    // 0 or 1

    /// 2-digit fields (one hex, one decimal — see encoder helpers).
    pub dirty_window: u8,     // 0..=99 (encoded as 2 hex digits)
    pub switch_off: u8,       // 2..=50 (encoded as 2 decimal digits)

    /// Live measurements — `*_x10` is the integer transported on the
    /// wire (1234.5 °C → 12345). The `ek` command reads `(one, ratio)`
    /// in one round-trip.
    pub measuring_value_x10: u32,
    pub ratio_temperature_x10: u32,
    pub one_channel_temperature_x10: u32,
    pub peak_value_x10: u32,
    pub internal_temp: u16, // 0..=98 °C / 32..=210 °F (integer)
    pub signal_strength: u16, // 0..=1500

    /// Identity / metadata.
    pub device_type: String,    // padded to 16 ASCII chars
    pub serial_number_hex: String, // 5 hex digits
    pub reference_number_hex: String, // 6 hex digits

    /// Range info — manual encodes lo / hi as 4 hex digits each.
    pub basic_range_lo: u16,
    pub basic_range_hi: u16,
    pub sub_range_lo: u16,
    pub sub_range_hi: u16,

    // ── Fault injection ────────────────────────────────────────────

    /// Sleep this long before sending the response. Use 6+ ms to
    /// make the client trip its 5 ms response-window timeout.
    pub delay_response_ms: u64,

    /// Skip the next response entirely. Bool counter — set true
    /// once, the simulator clears it after dropping one response.
    pub drop_next_response: bool,
}

impl IgarState {
    /// Sensible defaults that match the IGAR 6 Smart's factory
    /// settings (manual §4.1).
    pub fn factory_defaults() -> Self {
        Self {
            emissivity: 1000,
            transmittance: 1000,
            emissivity_ratio_k: 1000,
            response_time: 0,
            clear_peak_mode: 0,
            op_mode: 2, // ratio
            fahrenheit: 0,
            laser: 0,
            analog_output: 0, // 0..20 mA
            dirty_window: 0,
            switch_off: 10,
            measuring_value_x10: 12345, // 1234.5 °C — a recognisable test value
            ratio_temperature_x10: 12345,
            one_channel_temperature_x10: 12340,
            peak_value_x10: 12345,
            internal_temp: 35,
            signal_strength: 1500,
            device_type: "IGAR 6 Smart    ".into(), // 16 ASCII chars per manual §7
            serial_number_hex: "0AB1F".into(),
            reference_number_hex: "0039E0".into(),
            basic_range_lo: 0x00FA, // 250 dec — 250 °C
            basic_range_hi: 0x09C4, // 2500 dec — 2500 °C
            sub_range_lo: 0x00FA,
            sub_range_hi: 0x09C4,
            delay_response_ms: 0,
            drop_next_response: false,
        }
    }
}

/// A simulator process that holds one PTY end and serves one or
/// more "virtual devices" — each with its own address and state.
///
/// On a real RS485 bus several pyrometers share one wire and the
/// master's bytes reach all of them; only the device whose address
/// matches the request prefix replies. socat PTYs cannot be opened
/// twice, so multi-device bus topology is modelled by one simulator
/// process internally routing each frame to the matching virtual
/// device. Address filtering, broadcast distribution, and per-device
/// state isolation are all exercised by this layer.
pub struct IgarSimulator {
    /// Per-address state, keyed by the individual address (0..=97).
    states: Vec<(u8, Arc<Mutex<IgarState>>)>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl IgarSimulator {
    /// Spawn a single-address simulator. Convenience for the common
    /// case — equivalent to `spawn_multi(port, baud, &[address])`.
    pub fn spawn(port_path: &str, baud: u32, address: u8) -> Self {
        Self::spawn_multi(port_path, baud, &[address])
    }

    /// Spawn a simulator that listens for several addresses on one
    /// PTY end. Each address gets its own freshly-defaulted
    /// [`IgarState`]; tests use [`Self::state_of`] to mutate one
    /// device's state without touching the others.
    ///
    /// Use this when the test needs bus-level behaviour (cross-talk
    /// proof, broadcast reaching multiple devices). It is NOT a
    /// substitute for opening multiple OS serial-port handles —
    /// socat PTYs have a single slave and can only be opened once.
    pub fn spawn_multi(port_path: &str, baud: u32, addresses: &[u8]) -> Self {
        assert!(!addresses.is_empty(), "spawn_multi: need at least one address");
        let states: Vec<(u8, Arc<Mutex<IgarState>>)> = addresses
            .iter()
            .map(|&a| (a, Arc::new(Mutex::new(IgarState::factory_defaults()))))
            .collect();
        let stop = Arc::new(AtomicBool::new(false));

        let port_path = port_path.to_string();
        let states_clone = states.clone();
        let stop_clone = Arc::clone(&stop);

        let handle = std::thread::spawn(move || {
            run(port_path, baud, states_clone, stop_clone);
        });

        Self { states, stop, handle: Some(handle) }
    }

    /// Lock the state of the simulator's only address. Panics if
    /// the simulator was created with `spawn_multi` and more than
    /// one address — use [`Self::state_of`] instead.
    pub fn state(&self) -> std::sync::MutexGuard<'_, IgarState> {
        assert_eq!(
            self.states.len(),
            1,
            "state(): simulator has {} addresses, use state_of(addr)",
            self.states.len(),
        );
        self.states[0].1.lock().unwrap()
    }

    /// Lock the state of a specific virtual device by address.
    /// Panics if `address` was not registered in `spawn_multi`.
    pub fn state_of(&self, address: u8) -> std::sync::MutexGuard<'_, IgarState> {
        let entry = self
            .states
            .iter()
            .find(|(a, _)| *a == address)
            .unwrap_or_else(|| panic!("state_of: address {address} not registered"));
        entry.1.lock().unwrap()
    }

    /// Signal the worker thread to stop and wait for it to exit.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for IgarSimulator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

// ── Worker thread ──────────────────────────────────────────────────

fn run(
    port_path: String,
    baud: u32,
    states: Vec<(u8, Arc<Mutex<IgarState>>)>,
    stop: Arc<AtomicBool>,
) {
    let mut port = match serialport::new(&port_path, baud)
        .data_bits(DataBits::Eight)
        .parity(Parity::Even)
        .stop_bits(StopBits::One)
        .timeout(Duration::from_millis(50))
        .open()
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[IgarSimulator] cannot open {port_path}: {e}");
            return;
        }
    };

    let mut buf = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    while !stop.load(Ordering::SeqCst) {
        match port.read(&mut byte) {
            Ok(0) => continue,
            Ok(_) => {
                buf.push(byte[0]);
                if byte[0] == b'\r' {
                    handle_request(&mut *port, &buf, &states);
                    buf.clear();
                }
                if buf.len() > 64 {
                    // Defensive cap — discard a runaway stream
                    // (shouldn't happen on a healthy bus).
                    buf.clear();
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        }
    }
}

fn handle_request(
    port: &mut dyn SerialPort,
    frame: &[u8],
    states: &[(u8, Arc<Mutex<IgarState>>)],
) {
    // Frame must be at least "AAcc\r" — 5 bytes.
    if frame.len() < 5 || *frame.last().unwrap() != b'\r' {
        return;
    }
    let body = &frame[..frame.len() - 1]; // drop CR

    // Address.
    let a0 = body[0];
    let a1 = body[1];
    if !a0.is_ascii_digit() || !a1.is_ascii_digit() {
        return;
    }
    let req_addr = (a0 - b'0') * 10 + (a1 - b'0');

    // Route the frame to one or more virtual devices. Manual §4.14:
    // - 00..=97  → exactly the matching device
    // - 98       → "global with response, only one device on the bus"
    //              (but if N>1 each device replies — the bus collides;
    //              we model this faithfully by letting each registered
    //              device emit its frame back-to-back)
    // - 99       → every device applies, none responds
    let (targets, responder): (Vec<&Arc<Mutex<IgarState>>>, RouteKind) = match req_addr {
        99 => (
            states.iter().map(|(_, s)| s).collect(),
            RouteKind::ApplyOnly,
        ),
        98 => (
            states.iter().map(|(_, s)| s).collect(),
            RouteKind::AllRespondAsSelf,
        ),
        n => match states.iter().find(|(a, _)| *a == n) {
            Some((_, s)) => (vec![s], RouteKind::IndividualReply),
            None => return, // not for any of our virtual devices
        },
    };

    let Ok(cmd_str) = std::str::from_utf8(&body[2..]) else { return };

    for (i, state) in targets.iter().enumerate() {
        let response_payload = build_response(cmd_str, state, Route::from(responder));

        if matches!(responder, RouteKind::ApplyOnly) {
            continue; // 99 — mutate state, no wire bytes
        }

        // Identify which device is responding (its individual address).
        // For individual: it's req_addr. For 98: each registered device
        // in turn echoes its own address.
        let echo_addr = match responder {
            RouteKind::IndividualReply => req_addr,
            RouteKind::AllRespondAsSelf => states[i].0,
            RouteKind::ApplyOnly => unreachable!(),
        };

        let mut out = Vec::with_capacity(2 + response_payload.len() + 1);
        out.push(b'0' + echo_addr / 10);
        out.push(b'0' + echo_addr % 10);
        out.extend_from_slice(&response_payload);
        out.push(b'\r');

        // Fault injection — read flags off the responding device's state.
        let (delay, drop) = {
            let mut s = state.lock().unwrap();
            let d = s.delay_response_ms;
            let drop = s.drop_next_response;
            if drop {
                s.drop_next_response = false;
            }
            (d, drop)
        };
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }
        if drop {
            continue;
        }
        let _ = port.write_all(&out);
        let _ = port.flush();
    }
}

/// Per-frame routing decision — see [`handle_request`].
#[derive(Debug, Clone, Copy)]
enum RouteKind {
    /// Address 00..=97 — exactly one device replies.
    IndividualReply,
    /// Address 98 — every registered device replies with its own
    /// individual address (models real bus collision when N>1).
    AllRespondAsSelf,
    /// Address 99 — every device applies the write; nobody replies.
    ApplyOnly,
}

impl From<RouteKind> for Route {
    fn from(r: RouteKind) -> Self {
        match r {
            RouteKind::IndividualReply => Route::Individual,
            RouteKind::AllRespondAsSelf => Route::BroadcastWithResponse,
            RouteKind::ApplyOnly => Route::BroadcastNoResponse,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Route {
    Individual,
    BroadcastWithResponse,
    BroadcastNoResponse,
}

/// Dispatch on the 2-letter mnemonic, mutate state for writes, and
/// return the bytes that go between the address prefix and the CR.
fn build_response(cmd: &str, state: &Arc<Mutex<IgarState>>, _route: Route) -> Vec<u8> {
    let cmd_bytes = cmd.as_bytes();
    if cmd_bytes.len() < 2 {
        return Vec::new();
    }
    let mnem = &cmd[..2];
    let tail = &cmd[2..];

    // Limits queries: "?" tail.
    if tail == "?" {
        return limits_response(mnem, state).unwrap_or_default();
    }

    // No-tail: read.
    if tail.is_empty() {
        return read_response(mnem, state);
    }

    // Tailed: write. Most writes echo the new value (manual §7
    // worked example "00em0853 + CR"); pure-write commands return
    // "ok" / "no".
    write_response(mnem, tail, state)
}

fn read_response(mnem: &str, state: &Arc<Mutex<IgarState>>) -> Vec<u8> {
    let s = state.lock().unwrap();
    match mnem {
        // 4-digit / 1000 reads
        "em" => format!("{:04}", s.emissivity).into_bytes(),
        "et" => format!("{:04}", s.transmittance).into_bytes(),
        "ev" => format!("{:04}", s.emissivity_ratio_k).into_bytes(),
        "aw" => format!("{:02}", s.switch_off).into_bytes(),
        "dw" => format!("{:02X}", s.dirty_window).into_bytes(),

        // 1-digit selectors
        "ez" => vec![b'0' + s.response_time],
        "lz" => vec![b'0' + s.clear_peak_mode],
        "ka" => vec![b'0' + s.op_mode],
        "fh" => vec![b'0' + s.fahrenheit],
        "la" => vec![b'0' + s.laser],

        // 2-digit decimal device address
        "ga" => format!("{:02}", 0).into_bytes(), // simulator owns its own address; report 00 for now

        // Range info — 8 hex digits
        "mb" => format!("{:04X}{:04X}", s.basic_range_lo, s.basic_range_hi).into_bytes(),
        "me" => format!("{:04X}{:04X}", s.sub_range_lo, s.sub_range_hi).into_bytes(),

        // Measurements
        "ms" => format!("{:05}", s.measuring_value_x10).into_bytes(),
        "ek" => format!(
            "{:05}{:05}",
            s.one_channel_temperature_x10, s.ratio_temperature_x10
        )
        .into_bytes(),
        "tm" => format!("{:05}", s.peak_value_x10).into_bytes(),
        "gt" => format!("{:03}", s.internal_temp).into_bytes(),
        "tr" => format!("{:04}", s.signal_strength).into_bytes(),

        // Identity
        "na" => s.device_type.as_bytes().to_vec(),
        "sn" => s.serial_number_hex.as_bytes().to_vec(),
        "bn" => s.reference_number_hex.as_bytes().to_vec(),

        _ => Vec::new(), // unknown read — return an empty payload (decoder will error)
    }
}

fn write_response(mnem: &str, tail: &str, state: &Arc<Mutex<IgarState>>) -> Vec<u8> {
    let mut s = state.lock().unwrap();
    let ok = match mnem {
        "em" => parse_4digit(tail).map(|v| s.emissivity = v).is_some(),
        "et" => parse_4digit(tail).map(|v| s.transmittance = v).is_some(),
        "ev" => parse_4digit(tail).map(|v| s.emissivity_ratio_k = v).is_some(),
        "ez" => parse_1digit(tail).map(|v| s.response_time = v).is_some(),
        "lz" => parse_1digit(tail).map(|v| s.clear_peak_mode = v).is_some(),
        "ka" => parse_1digit(tail).map(|v| s.op_mode = v).is_some(),
        "fh" => parse_1digit(tail).map(|v| s.fahrenheit = v).is_some(),
        "la" => parse_1digit(tail).map(|v| s.laser = v).is_some(),
        "as" => parse_1digit(tail).map(|v| s.analog_output = v).is_some(),
        "aw" => parse_2digit_dec(tail).map(|v| s.switch_off = v).is_some(),
        "dw" => parse_2digit_hex(tail).map(|v| s.dirty_window = v).is_some(),
        // m1XXXXYYYY — sub-range step 1
        "m1" => {
            if tail.len() != 8 {
                false
            } else {
                let lo = u16::from_str_radix(&tail[0..4], 16);
                let hi = u16::from_str_radix(&tail[4..8], 16);
                match (lo, hi) {
                    (Ok(lo), Ok(hi)) => {
                        s.sub_range_lo = lo;
                        s.sub_range_hi = hi;
                        true
                    }
                    _ => false,
                }
            }
        }
        // Pure-writes that don't take a parameter — manual examples:
        // "lx" software clear-peak, "m2" confirm sub-range. They
        // arrive as `lx<somechar>` only if the user typed a wrong
        // body; we only handle the no-param form via the read path.
        _ => return Vec::new(),
    };

    // Manual §7 worked write example: "Answer: '00em0853' + CR" —
    // i.e. the mnemonic + new value is echoed back. We return the
    // same `mnem + tail` shape so the client's `strip_command_echo`
    // strips the mnemonic and decodes `tail`.
    if ok {
        let mut out = mnem.as_bytes().to_vec();
        out.extend_from_slice(tail.as_bytes());
        out
    } else {
        b"no".to_vec()
    }
}

fn limits_response(mnem: &str, _state: &Arc<Mutex<IgarState>>) -> Option<Vec<u8>> {
    // Per manual §7 worked example: `00em?` returns `00501000` —
    // the lo/hi pair as 8 hex digits (matching the basic-range
    // shape).
    let (lo, hi) = match mnem {
        "em" => (0x0050u16, 0x1000u16), // matches manual example exactly
        "et" => (0x0050, 0x1000),
        "ev" => (0x0800, 0x1200),
        "aw" => (0x0002, 0x0050),
        "dw" => (0x0000, 0x0099),
        "ez" => (0x0000, 0x0006),
        "lz" => (0x0000, 0x0009),
        "ka" => (0x0000, 0x0003),
        "ga" => (0x0000, 0x0099),
        "br" => (0x0000, 0x0008),
        _ => return None,
    };
    Some(format!("{lo:04X}{hi:04X}").into_bytes())
}

// ── Tail-parsing helpers ───────────────────────────────────────────

fn parse_4digit(s: &str) -> Option<u16> {
    if s.len() != 4 {
        return None;
    }
    s.parse::<u16>().ok()
}

fn parse_1digit(s: &str) -> Option<u8> {
    if s.len() != 1 {
        return None;
    }
    s.parse::<u8>().ok()
}

fn parse_2digit_dec(s: &str) -> Option<u8> {
    if s.len() != 2 {
        return None;
    }
    s.parse::<u8>().ok()
}

fn parse_2digit_hex(s: &str) -> Option<u8> {
    if s.len() != 2 {
        return None;
    }
    u8::from_str_radix(s, 16).ok()
}
