//! Phase 8 — `UppClient` ↔ `IgarSimulator` integration tests.
//!
//! These run the full UPP stack — `UppClient` → `SerialTransport` →
//! socat-PTY → `IgarSimulator` — and assert behaviour the unit tests
//! can't reach: real wire timing, the 5 ms response window, the
//! 1.5 ms post-response cooldown, retry recovery, and bus arbitration
//! with two devices on the same line.
//!
//! Gated by `ST_REQUIRE_SOCAT=1` per project policy: in CI we fail
//! loudly if `socat` is missing, locally we skip with a warning.
//! Mirrors the structure of
//! `crates/st-comm-modbus/tests/rtu_integration_test.rs` so reviewers
//! can navigate by analogy.

#[path = "igar_simulator.rs"]
mod igar_simulator;

use igar_simulator::IgarSimulator;
use st_comm_serial::transport::{ParityMode, SerialConfig, SerialTransport};
use st_comm_upp::{
    Address, Command, DecodedValue, Decoder, UppClient, UppError, UppResponse,
};
use std::process::{Child, Command as OsCommand};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TEST_BAUD: u32 = 19200;
const SLAVE_ADDRESS: u8 = 0;

// ── socat plumbing ────────────────────────────────────────────────

fn socat_available() -> bool {
    OsCommand::new("socat")
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
    let port_a = format!("/tmp/st-upp-int-a-{}-{suffix}", std::process::id());
    let port_b = format!("/tmp/st-upp-int-b-{}-{suffix}", std::process::id());
    let _ = std::fs::remove_file(&port_a);
    let _ = std::fs::remove_file(&port_b);

    let child = OsCommand::new("socat")
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

/// Open the master end as a `SerialTransport` configured for UPP
/// (8E1, 19200 by default). Wider timeout than the manual's 5 ms so
/// the transport itself isn't the limiter — the per-call timeout the
/// `UppClient` passes to `transaction_framed` is what governs the
/// 5 ms response-window contract.
fn open_master(port: &str) -> Arc<Mutex<SerialTransport>> {
    let config = SerialConfig {
        port: port.to_string(),
        baud_rate: TEST_BAUD,
        parity: ParityMode::Even,
        data_bits: 8,
        stop_bits: 1,
        timeout: Duration::from_millis(50),
    };
    let mut transport = SerialTransport::new(config);
    transport.open().expect("open master serial port");
    Arc::new(Mutex::new(transport))
}

fn ind(n: u8) -> Address {
    Address::individual(n).expect("individual address")
}

// ── Tests: read/write each command class ──────────────────────────

#[test]
fn read_emissivity_round_trip() {
    if !require_socat_or_skip("read_emissivity_round_trip") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("rd-em");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    let client = UppClient::new(open_master(&port_a));
    let (val, _) = client
        .transact(ind(SLAVE_ADDRESS), &Command::ReadEmissivity, Decoder::U16DecMilli)
        .expect("read emissivity");
    assert_eq!(val, DecodedValue::Per1000(1.000));

    sim.stop();
    let _ = socat.kill();
}

#[test]
fn read_measuring_value_round_trip() {
    if !require_socat_or_skip("read_measuring_value_round_trip") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("rd-ms");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    let client = UppClient::new(open_master(&port_a));
    let (val, _) = client
        .transact(
            ind(SLAVE_ADDRESS),
            &Command::ReadMeasuringValue,
            Decoder::Temp5dTenth,
        )
        .expect("read ms");
    // factory_defaults seeds 12345 → 1234.5 °C
    assert_eq!(val, DecodedValue::Temperature(1234.5));

    sim.stop();
    let _ = socat.kill();
}

#[test]
fn read_limits_round_trip() {
    if !require_socat_or_skip("read_limits_round_trip") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("rd-lim");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    let client = UppClient::new(open_master(&port_a));
    let (val, _) = client
        .transact(
            ind(SLAVE_ADDRESS),
            &Command::ReadLimits(st_comm_upp::command::LimitsTarget::Emissivity),
            Decoder::HexPair8,
        )
        .expect("read em?");
    // Manual §7 worked example: 0050..1000 hex → 80..4096 dec.
    assert_eq!(val, DecodedValue::HexPair { lo: 0x0050, hi: 0x1000 });

    sim.stop();
    let _ = socat.kill();
}

#[test]
fn write_emissivity_then_read_back() {
    if !require_socat_or_skip("write_emissivity_then_read_back") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("wr-em");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    let client = UppClient::new(open_master(&port_a));

    // Write — manual §7 worked example: 853 → ε = 0.853.
    let (echoed, _) = client
        .transact(
            ind(SLAVE_ADDRESS),
            &Command::WriteEmissivity { value: 853 },
            Decoder::U16DecMilli,
        )
        .expect("write em");
    assert_eq!(echoed, DecodedValue::Per1000(0.853));

    // Simulator state must have been mutated by the write.
    assert_eq!(sim.state().emissivity, 853);

    // Follow-up read — must observe the new value.
    let (read_back, _) = client
        .transact(
            ind(SLAVE_ADDRESS),
            &Command::ReadEmissivity,
            Decoder::U16DecMilli,
        )
        .expect("read em");
    assert_eq!(read_back, DecodedValue::Per1000(0.853));

    sim.stop();
    let _ = socat.kill();
}

#[test]
fn read_device_type_round_trip() {
    if !require_socat_or_skip("read_device_type_round_trip") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("rd-na");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    let client = UppClient::new(open_master(&port_a));
    let (val, _) = client
        .transact(ind(SLAVE_ADDRESS), &Command::ReadDeviceType, Decoder::Text)
        .expect("read na");
    let DecodedValue::Text(s) = val else {
        panic!("expected Text, got {val:?}");
    };
    assert_eq!(s.len(), 16, "manual §7: na returns exactly 16 ASCII chars");
    assert!(s.starts_with("IGAR 6 Smart"));

    sim.stop();
    let _ = socat.kill();
}

// ── Bus timing: cooldown is honoured between back-to-back txns ────

/// Manual §7 "Additional instruction for the RS485 interface" item 4
/// requires ≥ 1.5 ms between the master receiving a response and
/// sending the next request. We can't measure this directly inside
/// the client (it sleeps before returning), but we CAN verify the
/// observable effect: N consecutive transactions must take at least
/// (N-1) × cooldown wall-clock time.
#[test]
fn cooldown_separates_consecutive_transactions() {
    if !require_socat_or_skip("cooldown_separates_consecutive_transactions") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("cool");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    let client = UppClient::new(open_master(&port_a));

    let n = 10;
    let started = Instant::now();
    for _ in 0..n {
        client
            .transact(
                ind(SLAVE_ADDRESS),
                &Command::ReadEmissivity,
                Decoder::U16DecMilli,
            )
            .expect("read em");
    }
    let elapsed = started.elapsed();

    // 10 transactions × 2 ms default cooldown = ≥ 18 ms (the first
    // transaction does not need a leading cooldown, but each of the
    // remaining 9 does plus its own trailing one — so the floor is
    // 9 × 2 ms = 18 ms even ignoring the wire time).
    assert!(
        elapsed >= Duration::from_millis(18),
        "10 transactions completed in {elapsed:?} — cooldown not honoured?"
    );

    sim.stop();
    let _ = socat.kill();
}

// ── Negative paths: timeout, retry, address mismatch ──────────────

/// Manual §7: master must abort if no response within 5 ms. We
/// configure the simulator to take 50 ms — well past the deadline —
/// and assert the client classifies it as `UppError::Timeout`.
#[test]
fn delayed_response_triggers_timeout() {
    if !require_socat_or_skip("delayed_response_triggers_timeout") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("tmout");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);
    sim.state().delay_response_ms = 50;

    let client = UppClient::new(open_master(&port_a));
    let err = client
        .transaction(ind(SLAVE_ADDRESS), &Command::ReadEmissivity)
        .expect_err("expected timeout");
    assert!(
        matches!(err, UppError::Timeout),
        "expected UppError::Timeout, got {err:?}"
    );

    sim.stop();
    let _ = socat.kill();
}

/// First transaction's response is dropped → client times out.
/// Second transaction goes through normally → client recovers
/// without re-opening the port. Models the "single missed reply,
/// caller retries" pattern that's common on noisy buses.
#[test]
fn drop_then_recover() {
    if !require_socat_or_skip("drop_then_recover") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("drop");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);
    sim.state().drop_next_response = true;

    let client = UppClient::new(open_master(&port_a));

    // Drop one — must time out.
    let err = client
        .transaction(ind(SLAVE_ADDRESS), &Command::ReadEmissivity)
        .expect_err("first response was dropped");
    assert!(matches!(err, UppError::Timeout), "expected Timeout, got {err:?}");

    // Recover on the next attempt — same client, same transport.
    let (val, _) = client
        .transact(
            ind(SLAVE_ADDRESS),
            &Command::ReadEmissivity,
            Decoder::U16DecMilli,
        )
        .expect("retry must succeed");
    assert_eq!(val, DecodedValue::Per1000(1.000));

    sim.stop();
    let _ = socat.kill();
}

// ── Multi-device scenarios ────────────────────────────────────────

/// Bus topology with two virtual devices (addresses 1 and 2)
/// hosted by one simulator process — socat PTYs cannot be opened
/// twice, so multi-device behaviour is modelled in-process. The
/// simulator's address-filtering routes each frame to the correct
/// device's state. Alternate reads must observe each device's
/// distinct measuring value with zero cross-talk.
#[test]
fn two_devices_alternating_reads_no_crosstalk() {
    if !require_socat_or_skip("two_devices_alternating_reads_no_crosstalk") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("multi");
    let sim = IgarSimulator::spawn_multi(&port_b, TEST_BAUD, &[1, 2]);
    sim.state_of(1).measuring_value_x10 = 11111;
    sim.state_of(2).measuring_value_x10 = 22222;

    let client = UppClient::new(open_master(&port_a));
    for _ in 0..3 {
        let (v1, _) = client
            .transact(ind(1), &Command::ReadMeasuringValue, Decoder::Temp5dTenth)
            .expect("read addr 1");
        assert_eq!(v1, DecodedValue::Temperature(1111.1));

        let (v2, _) = client
            .transact(ind(2), &Command::ReadMeasuringValue, Decoder::Temp5dTenth)
            .expect("read addr 2");
        assert_eq!(v2, DecodedValue::Temperature(2222.2));
    }

    sim.stop();
    let _ = socat.kill();
}

/// Manual §4.14: address 99 = "global broadcast without response".
/// All devices on the bus apply the write; none transmit. The
/// client returns `UppResponse::NoResponse` immediately.
#[test]
fn broadcast_99_pushes_to_all_devices() {
    if !require_socat_or_skip("broadcast_99_pushes_to_all_devices") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("b99");
    let sim = IgarSimulator::spawn_multi(&port_b, TEST_BAUD, &[0, 1]);
    assert_eq!(sim.state_of(0).emissivity, 1000);
    assert_eq!(sim.state_of(1).emissivity, 1000);

    let client = UppClient::new(open_master(&port_a));
    let (resp, _) = client
        .transaction(
            Address::BroadcastNoResponse,
            &Command::WriteEmissivity { value: 750 },
        )
        .expect("broadcast must not error");
    assert!(matches!(resp, UppResponse::NoResponse));

    // The simulator applies the write synchronously inside its
    // request handler, but that runs on a background thread — give
    // it a moment to pick up the bytes that were just flushed.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(sim.state_of(0).emissivity, 750);
    assert_eq!(sim.state_of(1).emissivity, 750);

    sim.stop();
    let _ = socat.kill();
}

/// Manual §4.14: address 98 = "global broadcast with response —
/// only one device on the bus". With one device on the line the
/// master gets a normal-shaped reply; the wire prefix carries the
/// responding device's INDIVIDUAL address (not 98), and the client
/// accepts it because `addresses_match` special-cases broadcast 98.
#[test]
fn broadcast_98_single_device_responds_with_individual_address() {
    if !require_socat_or_skip("broadcast_98_single_device_responds_with_individual_address") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("b98");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, 7);

    let client = UppClient::new(open_master(&port_a));
    let (resp, _) = client
        .transaction(Address::BroadcastWithResponse, &Command::ReadEmissivity)
        .expect("broadcast 98 must succeed with one device");
    let UppResponse::Frame { address, payload } = resp else {
        panic!("expected Frame, got {resp:?}");
    };
    assert_eq!(
        address.as_u8(),
        7,
        "response must carry the responding device's individual address"
    );
    assert_eq!(payload, b"1000");

    sim.stop();
    let _ = socat.kill();
}

/// Two devices both reply to a broadcast 98 — manual §4.14 says
/// "only one device on the bus" for this addressing mode. The
/// simulator emits both frames back-to-back (modelling the wire-
/// level collision). The client must classify the result cleanly
/// (BadResponse, AddressMismatch, or Timeout) and stay usable for
/// subsequent transactions.
#[test]
fn broadcast_98_collision_is_classified_not_crash() {
    if !require_socat_or_skip("broadcast_98_collision_is_classified_not_crash") {
        return;
    }
    let (mut socat, port_a, port_b) = spawn_virtual_serial("b98col");
    let sim = IgarSimulator::spawn_multi(&port_b, TEST_BAUD, &[1, 2]);

    let client = UppClient::new(open_master(&port_a));
    let res = client.transaction(Address::BroadcastWithResponse, &Command::ReadEmissivity);

    // The two devices' frames arrive back-to-back. Depending on
    // which frame the parser locks onto first, we get one of:
    //   - Ok(Frame{addr=1, …})  — client got the first frame and
    //     stopped at its CR; the second device's frame remains in
    //     the OS input buffer and we drain it before the next
    //     transaction
    //   - BadResponse / AddressMismatch — interleaving produced a
    //     malformed frame
    //   - Timeout — no CR within the 5 ms window
    //
    // Any of these is acceptable; what matters is that the client
    // doesn't crash and stays usable.
    match res {
        Ok(_) | Err(UppError::BadResponse(_)) | Err(UppError::AddressMismatch { .. })
        | Err(UppError::Timeout) => {}
        Err(other) => panic!("unexpected error class: {other:?}"),
    }

    // After the collision the second frame may still be sitting in
    // the OS receive buffer. The next transaction would mistake
    // those leftover bytes for its own response — the transport's
    // input flush at transaction start handles that case for us.
    let (v1, _) = client
        .transact(ind(1), &Command::ReadEmissivity, Decoder::U16DecMilli)
        .expect("client must recover for individual reads");
    assert_eq!(v1, DecodedValue::Per1000(1.000));

    sim.stop();
    let _ = socat.kill();
}
