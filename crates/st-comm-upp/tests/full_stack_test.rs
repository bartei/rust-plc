//! Phase 9 — full-stack E2E: ST program → compiler → engine → SerialLink
//! → UppDeviceNativeFb → socat → IgarSimulator.
//!
//! Compiles a real ST program that declares an UPP-protocol device,
//! drives the in-process VM through several scan cycles, and asserts
//! that:
//!
//! - the simulator's measured value flows into an ST global
//! - `connected = TRUE` and `errors_count` stays at 0 on a healthy bus
//! - an ST-side write of `emissivity := 0.853` reaches the simulator
//!
//! Mirrors `crates/st-comm-modbus/tests/full_stack_test.rs` so the
//! reviewer can navigate by analogy. Gated on `socat`.

#[path = "igar_simulator.rs"]
mod igar_simulator;

use igar_simulator::IgarSimulator;
use st_comm_api::{DeviceProfile, NativeFbRegistry};
use st_comm_upp::UppDeviceNativeFb;
use st_ir::Value;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;

const TEST_BAUD: u32 = 19200;
const SLAVE_ADDRESS: u8 = 0;

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
    let port_a = format!("/tmp/st-upp-fs-a-{}-{suffix}", std::process::id());
    let port_b = format!("/tmp/st-upp-fs-b-{}-{suffix}", std::process::id());
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

/// Trimmed UPP profile — enough to exercise the read/write paths
/// end-to-end without hauling in the full 19-field IGAR profile.
/// Field types match what the runtime's `decoded_to_value` mapping
/// expects: REAL for /1000 and tenths-temperature decoders.
const UPP_PROFILE_YAML: &str = r#"
name: IgarPyro
protocol: upp
fields:
  - name: temperature
    type: REAL
    direction: input
    upp:
      command: ms
      decoder: temp_5d_tenth

  - name: emissivity
    type: REAL
    direction: inout
    upp:
      command: em
      decoder: u16_dec_milli
"#;

#[test]
fn full_stack_upp_through_vm() {
    if !require_socat_or_skip("full_stack_upp_through_vm") {
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial("e2e");
    let sim = IgarSimulator::spawn(&port_b, TEST_BAUD, SLAVE_ADDRESS);

    // Seed a recognisable measurement: 1234.5 °C → 12345 on the wire.
    sim.state().measuring_value_x10 = 12345;

    // Build the registry: SerialLink + UppDevice share one transport
    // map / bus manager (the runtime's standard two-layer wiring).
    let profile = DeviceProfile::from_yaml(UPP_PROFILE_YAML).expect("parse upp profile");
    let transport_map = st_comm_serial::new_transport_map();
    let bus_manager = Arc::new(st_comm_serial::BusManager::new(Arc::clone(&transport_map)));
    let mut registry = NativeFbRegistry::new();
    registry.register(Box::new(
        st_comm_serial::SerialLinkNativeFb::with_transport_map(Arc::clone(&transport_map)),
    ));
    registry.register(Box::new(UppDeviceNativeFb::new(
        profile,
        Arc::clone(&bus_manager),
    )));

    // ST program: open a SerialLink at 19200 8E1, instantiate the
    // UPP device against it, and copy live values into globals so we
    // can assert on them. The cycle also writes ε := 0.853 to verify
    // round-trip writes reach the simulator.
    let dev = SLAVE_ADDRESS;
    let source = format!(
        r#"
VAR_GLOBAL
    g_connected   : BOOL := FALSE;
    g_temperature : REAL := 0.0;
    g_errors      : UDINT := 0;
    g_eps_set     : BOOL := FALSE;
END_VAR

PROGRAM Main
VAR
    serial : SerialLink;
    pyro   : IgarPyro;
END_VAR
    serial(
        port := '{port_a}',
        baud := 19200,
        parity := 'E',
        data_bits := 8,
        stop_bits := 1
    );

    pyro(
        link := serial.port,
        device_id := {dev},
        refresh_rate := T#0ms
    );

    g_connected := pyro.connected;
    g_temperature := pyro.temperature;
    g_errors := pyro.errors_count;

    pyro.emissivity := 0.853;
    g_eps_set := TRUE;
END_PROGRAM
"#
    );

    // Compile against stdlib + native FB registry.
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(&source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    assert!(
        parse_result.errors.is_empty(),
        "Parse errors: {:?}",
        parse_result.errors
    );

    let module =
        st_compiler::compile_with_native_fbs(&parse_result.source_file, Some(&registry))
            .expect("compile");

    let arc_reg = Arc::new(registry);
    let mut vm = st_engine::vm::Vm::new_with_native_fbs(
        module,
        st_engine::vm::VmConfig::default(),
        Some(arc_reg),
    );
    let _ = vm.run_global_init();

    // First scan kicks off the BusManager I/O thread; subsequent
    // scans pick up cached values. Wait for the background poll to
    // complete a few cycles before asserting (UPP transactions are
    // ~5 ms each, but cooldowns and OS scheduling on a busy CI
    // runner can add latency — be generous).
    vm.scan_cycle("Main").expect("first scan");
    std::thread::sleep(Duration::from_millis(500));
    for _ in 0..5 {
        vm.scan_cycle("Main").expect("scan");
        std::thread::sleep(Duration::from_millis(50));
    }

    // Asserts.
    assert_eq!(
        vm.get_global("g_connected"),
        Some(&Value::Bool(true)),
        "device must be connected after a successful read"
    );
    assert_eq!(
        vm.get_global("g_temperature"),
        Some(&Value::Real(1234.5)),
        "ms temperature must flow through to the ST global (12345 → 1234.5)"
    );
    assert_eq!(
        vm.get_global("g_errors"),
        Some(&Value::UInt(0)),
        "no errors expected on a healthy bus"
    );

    // The ST program wrote ε := 0.853 every cycle; over a few cycles
    // the BusManager queue must have flushed at least one write.
    // Wait long enough for any in-flight write to complete.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        sim.state().emissivity,
        853,
        "ST write of 0.853 must have reached the simulator (got {})",
        sim.state().emissivity,
    );

    sim.stop();
    let _ = socat.kill();
}
