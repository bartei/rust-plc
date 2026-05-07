//! Self-tests for the [`IgarSimulator`] test helper.
//!
//! These prove the simulator IS the UPP spec — independently of the
//! `UppClient`. We send raw byte sequences from the manual's worked
//! examples on one PTY end and assert the simulator emits the
//! manual's exact answer bytes on the other.
//!
//! Without this layer, a bug in the simulator could invisibly mask
//! a bug in the client during Phase 8 integration tests. So this
//! file is the simulator's executable specification, the same way
//! `command::tests` and `parser::tests` pin the encoder / decoder
//! against manual §7.
//!
//! Gated by `ST_REQUIRE_SOCAT=1` per the project policy: in CI we
//! fail loudly if `socat` isn't on PATH; locally we skip with a
//! warning.

#[path = "igar_simulator.rs"]
mod igar_simulator;

use igar_simulator::IgarSimulator;
use serialport::{DataBits, Parity, StopBits};
use std::io::{Read, Write};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const TEST_BAUD: u32 = 19200;
const TEST_ADDRESS: u8 = 0;

// ── socat plumbing (mirrors crates/st-comm-modbus/tests/...) ─────

fn socat_available() -> bool {
    Command::new("socat")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn require_socat_or_skip(test_name: &str) -> bool {
    if socat_available() {
        return true;
    }
    let required = std::env::var("ST_REQUIRE_SOCAT")
        .map(|v| v == "1")
        .unwrap_or(false);
    if required {
        panic!(
            "{test_name}: socat required (ST_REQUIRE_SOCAT=1) but not on PATH. \
             Install via apt-get install -y socat (CI) or nix-shell -p socat (local)."
        );
    }
    eprintln!("Skipping {test_name} (socat not available)");
    false
}

fn spawn_virtual_serial(suffix: &str) -> (Child, String, String) {
    let port_a = format!("/tmp/st-upp-a-{}-{suffix}", std::process::id());
    let port_b = format!("/tmp/st-upp-b-{}-{suffix}", std::process::id());
    let _ = std::fs::remove_file(&port_a);
    let _ = std::fs::remove_file(&port_b);

    let child = Command::new("socat")
        .args([
            &format!("pty,raw,echo=0,link={port_a}"),
            &format!("pty,raw,echo=0,link={port_b}"),
        ])
        .spawn()
        .expect("Failed to spawn socat");

    for _ in 0..50 {
        if std::path::Path::new(&port_a).exists() && std::path::Path::new(&port_b).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        std::path::Path::new(&port_a).exists(),
        "socat didn't create {port_a}"
    );
    (child, port_a, port_b)
}

/// Send `request` and read until either `\r` arrives or the
/// per-call deadline expires. Returns the bytes received (no `\r`
/// stripping — the test asserts on the full frame).
fn round_trip(
    request: &[u8],
    port_path: &str,
    deadline: Duration,
) -> Result<Vec<u8>, String> {
    let mut port = serialport::new(port_path, TEST_BAUD)
        .data_bits(DataBits::Eight)
        .parity(Parity::Even)
        .stop_bits(StopBits::One)
        .timeout(Duration::from_millis(50))
        .open()
        .map_err(|e| format!("open client port: {e}"))?;
    port.write_all(request)
        .map_err(|e| format!("write: {e}"))?;
    port.flush().map_err(|e| format!("flush: {e}"))?;

    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    let start = Instant::now();
    while start.elapsed() < deadline {
        match port.read(&mut byte) {
            Ok(0) => continue,
            Ok(_) => {
                out.push(byte[0]);
                if byte[0] == b'\r' {
                    return Ok(out);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    Err(format!(
        "deadline exceeded after {} bytes: {:?}",
        out.len(),
        String::from_utf8_lossy(&out)
    ))
}

// ── Tests ─────────────────────────────────────────────────────────

/// Manual §7 worked example: `00em` request returns ε answer.
/// Default is 1.000 → `1000`. Our default in `factory_defaults()` is
/// 1000, so the wire frame is `001000\r`.
#[test]
fn read_emissivity_returns_factory_default() {
    if !require_socat_or_skip("read_emissivity_returns_factory_default") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("e1");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    let resp = round_trip(b"00em\r", &port_a, Duration::from_millis(500))
        .expect("round trip");
    assert_eq!(
        resp,
        b"001000\r",
        "factory ε is 1.000 → wire bytes 001000\\r; got {:?}",
        String::from_utf8_lossy(&resp)
    );

    sim.stop();
    let _ = socat.kill();
}

/// Manual §7 worked write example: `00em0853\r` request →
/// `00em0853\r` echo, AND state mutates so a follow-up read sees 853.
#[test]
fn write_emissivity_echoes_and_persists() {
    if !require_socat_or_skip("write_emissivity_echoes_and_persists") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("w1");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    let resp = round_trip(b"00em0853\r", &port_a, Duration::from_millis(500))
        .expect("write round trip");
    assert_eq!(
        resp,
        b"00em0853\r",
        "manual §7 write example: response echoes mnemonic + new value"
    );

    // State must have been mutated.
    assert_eq!(sim.state().emissivity, 853);

    // Follow-up read returns the new value (without echo).
    let read_back = round_trip(b"00em\r", &port_a, Duration::from_millis(500))
        .expect("read round trip");
    assert_eq!(read_back, b"000853\r");

    sim.stop();
    let _ = socat.kill();
}

/// Manual §7 limits-query example: `00em?` → `00501000`.
#[test]
fn limits_query_emissivity_matches_manual() {
    if !require_socat_or_skip("limits_query_emissivity_matches_manual") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("lim");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    let resp = round_trip(b"00em?\r", &port_a, Duration::from_millis(500))
        .expect("limits round trip");
    assert_eq!(resp, b"0000501000\r"); // address + 0050 lo + 1000 hi + CR

    sim.stop();
    let _ = socat.kill();
}

/// `ms` returns the 5-digit /10 measurement. Default state seeds
/// 12345 → 1234.5 °C.
#[test]
fn read_measuring_value_returns_5_digits() {
    if !require_socat_or_skip("read_measuring_value_returns_5_digits") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("ms");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    let resp = round_trip(b"00ms\r", &port_a, Duration::from_millis(500)).unwrap();
    assert_eq!(resp, b"0012345\r");

    sim.stop();
    let _ = socat.kill();
}

/// `ek` returns 10 digits — one_channel + ratio.
#[test]
fn read_pair_returns_10_digits() {
    if !require_socat_or_skip("read_pair_returns_10_digits") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("ek");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    let resp = round_trip(b"00ek\r", &port_a, Duration::from_millis(500)).unwrap();
    // factory: one_channel = 12340, ratio = 12345
    assert_eq!(resp, b"001234012345\r");

    sim.stop();
    let _ = socat.kill();
}

/// `na` returns the 16-char ASCII device-type string.
#[test]
fn read_device_type_text() {
    if !require_socat_or_skip("read_device_type_text") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("na");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    let resp = round_trip(b"00na\r", &port_a, Duration::from_millis(500)).unwrap();
    // address (2) + 16-char text + CR = 19 bytes.
    assert_eq!(resp.len(), 19);
    assert!(
        resp.starts_with(b"00IGAR 6 Smart  "),
        "expected device-type text after address prefix, got {:?}",
        String::from_utf8_lossy(&resp)
    );
    assert_eq!(*resp.last().unwrap(), b'\r');

    sim.stop();
    let _ = socat.kill();
}

/// Manual §4.14: address 99 = "global address without response".
/// The simulator must apply the write but emit no bytes.
#[test]
fn broadcast_99_applies_without_response() {
    if !require_socat_or_skip("broadcast_99_applies_without_response") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("b99");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    // No response expected — short deadline returns Err, which is the
    // correct outcome for broadcast 99.
    let resp = round_trip(b"99em0500\r", &port_a, Duration::from_millis(150));
    assert!(
        resp.is_err(),
        "broadcast 99 must not produce a response, got {resp:?}"
    );

    // …but state must still be mutated.
    assert_eq!(sim.state().emissivity, 500);

    sim.stop();
    let _ = socat.kill();
}

/// Manual §4.14: address 98 = "global address with response". The
/// device responds with its own individual address, NOT 98.
#[test]
fn broadcast_98_response_carries_individual_address() {
    if !require_socat_or_skip("broadcast_98_response_carries_individual_address") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("b98");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, 7);

    let resp = round_trip(b"98em\r", &port_a, Duration::from_millis(500)).unwrap();
    // Simulator's individual address is 7 → wire prefix "07".
    assert!(
        resp.starts_with(b"07"),
        "broadcast-with-response answer must echo the responding device's individual address, got {:?}",
        String::from_utf8_lossy(&resp)
    );
    assert!(*resp.last().unwrap() == b'\r');

    sim.stop();
    let _ = socat.kill();
}

/// Wrong individual address — the simulator must ignore the request.
#[test]
fn other_individual_address_is_ignored() {
    if !require_socat_or_skip("other_individual_address_is_ignored") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("ign");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, 0);

    // Address 5 — not us.
    let resp = round_trip(b"05em\r", &port_a, Duration::from_millis(150));
    assert!(
        resp.is_err(),
        "non-matching individual address must not produce a response, got {resp:?}"
    );

    sim.stop();
    let _ = socat.kill();
}

/// Fault injection: `delay_response_ms` must actually delay the
/// outbound bytes. Used to test client-side timeout in Phase 8.
#[test]
fn delay_response_ms_delays_the_answer() {
    if !require_socat_or_skip("delay_response_ms_delays_the_answer") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("delay");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    sim.state().delay_response_ms = 50;

    let t0 = Instant::now();
    let resp = round_trip(b"00em\r", &port_a, Duration::from_millis(500)).unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(resp, b"001000\r");
    assert!(
        elapsed >= Duration::from_millis(45),
        "response should have been delayed ~50 ms, observed {elapsed:?}"
    );

    sim.stop();
    let _ = socat.kill();
}

/// Fault injection: `drop_next_response` must swallow exactly one
/// response, then resume normally.
#[test]
fn drop_next_response_consumes_one_then_recovers() {
    if !require_socat_or_skip("drop_next_response_consumes_one_then_recovers") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("drop");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, TEST_ADDRESS);

    sim.state().drop_next_response = true;

    // First request: response is dropped — short read times out.
    let first = round_trip(b"00em\r", &port_a, Duration::from_millis(150));
    assert!(first.is_err(), "first response must be dropped, got {first:?}");

    // Second request: simulator resumes normally.
    let second = round_trip(b"00em\r", &port_a, Duration::from_millis(500))
        .expect("simulator must answer the second request");
    assert_eq!(second, b"001000\r");

    sim.stop();
    let _ = socat.kill();
}
