//! End-to-end tests for multi-rate I/O scheduling.
//!
//! Verifies that devices with different `cycle_time` settings are polled at
//! the correct rates when running through the full pipeline:
//! config parsing → ST codegen → compilation → engine execution.

use st_comm_api::{generate_st_code, DeviceConfig, DeviceProfile};
use st_comm_sim::SimulatedDevice;
use st_engine::*;
use st_ir::PouKind;
use std::collections::HashMap;
use std::time::Duration;

/// Load a bundled device profile from the `profiles/` directory.
fn load_profile(name: &str) -> DeviceProfile {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("profiles")
        .join(format!("{name}.yaml"));
    DeviceProfile::from_file(&path)
        .unwrap_or_else(|e| panic!("Failed to load profile '{name}': {e}"))
}

/// Build device configs for a fast device (every cycle) and a slow device (with cycle_time).
fn two_device_configs(slow_cycle_time: &str) -> (Vec<DeviceConfig>, HashMap<String, DeviceProfile>) {
    let profile = load_profile("sim_8di_4ai_4do_2ao");
    let mut profiles = HashMap::new();
    profiles.insert("sim_8di_4ai_4do_2ao".to_string(), profile);

    let devices = vec![
        DeviceConfig {
            name: "fast".to_string(),
            link: "sim_link".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: None,
            device_profile: "sim_8di_4ai_4do_2ao".to_string(),
            extra: Default::default(),
        },
        DeviceConfig {
            name: "slow".to_string(),
            link: "sim_link".to_string(),
            protocol: "simulated".to_string(),
            unit_id: None,
            mode: "cyclic".to_string(),
            cycle_time: Some(slow_cycle_time.to_string()),
            device_profile: "sim_8di_4ai_4do_2ao".to_string(),
            extra: Default::default(),
        },
    ];

    (devices, profiles)
}

/// Compile ST source with device I/O map and create an engine.
fn build_engine(
    st_source: &str,
    devices: &[DeviceConfig],
    profiles: &HashMap<String, DeviceProfile>,
    engine_cycle_time: Option<Duration>,
    max_cycles: u64,
) -> Engine {
    let io_map = generate_st_code(profiles, devices);
    let full_source = format!("{io_map}\n{st_source}");

    let parse_result = st_syntax::parse(&full_source);
    assert!(
        parse_result.errors.is_empty(),
        "Parse errors: {:?}",
        parse_result.errors
    );

    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");
    let program_name = module
        .functions
        .iter()
        .find(|f| f.kind == PouKind::Program)
        .expect("No PROGRAM found")
        .name
        .clone();

    let config = EngineConfig {
        max_cycles,
        cycle_time: engine_cycle_time,
        ..Default::default()
    };

    Engine::new(module, program_name, config)
}

// =============================================================================
// Tests
// =============================================================================

/// Verify that a device with no cycle_time is polled every scan cycle,
/// while a device with cycle_time is polled less frequently.
#[test]
fn fast_device_polled_every_cycle_slow_device_skipped() {
    let (devices, profiles) = two_device_configs("200ms");

    let source = r#"
PROGRAM Main
VAR
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
END_PROGRAM
"#;

    // Engine runs as fast as possible (no sleep), so all cycles complete
    // in well under 200ms — the slow device should only run once (first cycle).
    let mut engine = build_engine(source, &devices, &profiles, None, 0);

    let fast_profile = profiles["sim_8di_4ai_4do_2ao"].clone();
    let slow_profile = profiles["sim_8di_4ai_4do_2ao"].clone();
    let fast_dev = SimulatedDevice::new("fast", fast_profile);
    let slow_dev = SimulatedDevice::new("slow", slow_profile);

    engine.register_comm_device(Box::new(fast_dev), "fast", None);
    engine.register_comm_device(Box::new(slow_dev), "slow", Some(Duration::from_millis(200)));

    // Run 50 cycles as fast as possible (sub-millisecond total)
    for _ in 0..50 {
        engine.run_one_cycle().unwrap();
    }

    assert_eq!(engine.stats().cycle_count, 50);

    let diags = engine.comm().device_diagnostics();
    let fast_cycles = diags.iter().find(|(n, _)| *n == "fast").unwrap().1.successful_cycles;
    let slow_cycles = diags.iter().find(|(n, _)| *n == "slow").unwrap().1.successful_cycles;

    assert_eq!(fast_cycles, 50, "Fast device should run every cycle");
    assert_eq!(slow_cycles, 1, "Slow device should run only on the first cycle (all 50 cycles complete in <200ms)");
}

/// Verify that a slow device actually runs again after its cycle_time elapses.
#[test]
fn slow_device_runs_again_after_interval() {
    let (devices, profiles) = two_device_configs("50ms");

    let source = r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
END_PROGRAM
"#;

    let mut engine = build_engine(source, &devices, &profiles, None, 0);

    let fast_dev = SimulatedDevice::new("fast", profiles["sim_8di_4ai_4do_2ao"].clone());
    let slow_dev = SimulatedDevice::new("slow", profiles["sim_8di_4ai_4do_2ao"].clone());

    engine.register_comm_device(Box::new(fast_dev), "fast", None);
    engine.register_comm_device(Box::new(slow_dev), "slow", Some(Duration::from_millis(50)));

    // First cycle: both run
    engine.run_one_cycle().unwrap();
    let diags = engine.comm().device_diagnostics();
    let slow_cycles = diags.iter().find(|(n, _)| *n == "slow").unwrap().1.successful_cycles;
    assert_eq!(slow_cycles, 1, "Slow device should run on first cycle");

    // Run a few more cycles immediately — slow should stay at 1
    for _ in 0..5 {
        engine.run_one_cycle().unwrap();
    }
    let diags = engine.comm().device_diagnostics();
    let slow_cycles = diags.iter().find(|(n, _)| *n == "slow").unwrap().1.successful_cycles;
    assert_eq!(slow_cycles, 1, "Slow device should still be at 1 (50ms not elapsed)");

    // Sleep past the cycle_time
    std::thread::sleep(Duration::from_millis(60));

    // Next cycle should trigger the slow device again
    engine.run_one_cycle().unwrap();
    let diags = engine.comm().device_diagnostics();
    let slow_cycles = diags.iter().find(|(n, _)| *n == "slow").unwrap().1.successful_cycles;
    assert_eq!(slow_cycles, 2, "Slow device should run after 50ms elapsed");
}

/// Verify multi-rate scheduling with real engine cycle_time.
/// Engine runs at 10ms cycle time, slow device at 50ms — after 100ms the slow
/// device should have run approximately 2 times.
#[test]
fn multi_rate_with_engine_cycle_time() {
    let (devices, profiles) = two_device_configs("50ms");

    let source = r#"
PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
END_PROGRAM
"#;

    let mut engine = build_engine(
        source,
        &devices,
        &profiles,
        Some(Duration::from_millis(10)), // 10ms engine cycle
        10,                               // run 10 cycles = ~100ms
    );

    let fast_dev = SimulatedDevice::new("fast", profiles["sim_8di_4ai_4do_2ao"].clone());
    let slow_dev = SimulatedDevice::new("slow", profiles["sim_8di_4ai_4do_2ao"].clone());

    engine.register_comm_device(Box::new(fast_dev), "fast", None);
    engine.register_comm_device(Box::new(slow_dev), "slow", Some(Duration::from_millis(50)));

    engine.run().unwrap();

    assert_eq!(engine.stats().cycle_count, 10);

    let diags = engine.comm().device_diagnostics();
    let fast_cycles = diags.iter().find(|(n, _)| *n == "fast").unwrap().1.successful_cycles;
    let slow_cycles = diags.iter().find(|(n, _)| *n == "slow").unwrap().1.successful_cycles;

    // Fast device runs every cycle
    assert_eq!(fast_cycles, 10, "Fast device should run every cycle");

    // Slow device: first at t=0, then at ~50ms, then at ~100ms.
    // With 10ms cycles over 100ms, that's approximately 2-3 slow cycles.
    assert!(
        slow_cycles >= 2 && slow_cycles <= 3,
        "Slow device should run 2-3 times in 100ms at 50ms intervals, got {slow_cycles}"
    );
}

/// Verify that device globals hold last-known values between slow device cycles.
/// The fast device's DI_0 changes every cycle, but the slow device's input
/// globals should be stale between updates.
#[test]
fn slow_device_globals_hold_last_known_values() {
    let (devices, profiles) = two_device_configs("200ms");

    let source = r#"
PROGRAM Main
VAR
    fast_in : BOOL;
    slow_in : BOOL;
END_VAR
    fast_in := fast_DI_0;
    slow_in := slow_DI_0;
END_PROGRAM
"#;

    let mut engine = build_engine(source, &devices, &profiles, None, 0);

    let fast_dev = SimulatedDevice::new("fast", profiles["sim_8di_4ai_4do_2ao"].clone());
    let slow_dev = SimulatedDevice::new("slow", profiles["sim_8di_4ai_4do_2ao"].clone());

    // Set initial input values
    fast_dev
        .set_input("DI_0", st_comm_api::IoValue::Bool(true))
        .unwrap();
    slow_dev
        .set_input("DI_0", st_comm_api::IoValue::Bool(true))
        .unwrap();

    engine.register_comm_device(Box::new(fast_dev), "fast", None);
    engine.register_comm_device(Box::new(slow_dev), "slow", Some(Duration::from_millis(200)));

    // First cycle: both devices read, so both globals are TRUE
    engine.run_one_cycle().unwrap();
    assert_eq!(
        engine.vm().get_global("slow_DI_0"),
        Some(&st_ir::Value::Bool(true)),
        "Slow device DI_0 should be TRUE after first cycle"
    );

    // Run 10 more cycles rapidly — slow device is NOT read again.
    // The global should still hold the last-known TRUE value.
    for _ in 0..10 {
        engine.run_one_cycle().unwrap();
    }
    assert_eq!(
        engine.vm().get_global("slow_DI_0"),
        Some(&st_ir::Value::Bool(true)),
        "Slow device DI_0 should hold last-known value between updates"
    );
}

/// Verify that outputs are only written to devices that were read in the same cycle.
#[test]
fn outputs_only_written_when_device_read() {
    let (devices, profiles) = two_device_configs("200ms");

    let source = r#"
PROGRAM Main
VAR END_VAR
    fast_DO_0 := TRUE;
    slow_DO_0 := TRUE;
END_PROGRAM
"#;

    let mut engine = build_engine(source, &devices, &profiles, None, 0);

    let fast_dev = SimulatedDevice::new("fast", profiles["sim_8di_4ai_4do_2ao"].clone());
    let slow_dev = SimulatedDevice::new("slow", profiles["sim_8di_4ai_4do_2ao"].clone());

    let slow_state = slow_dev.state_handle();

    engine.register_comm_device(Box::new(fast_dev), "fast", None);
    engine.register_comm_device(Box::new(slow_dev), "slow", Some(Duration::from_millis(200)));

    // First cycle: both devices are written to
    engine.run_one_cycle().unwrap();
    {
        let state = slow_state.lock().unwrap();
        assert_eq!(
            state.get("DO_0"),
            Some(&st_comm_api::IoValue::Bool(true)),
            "Slow device DO_0 should be written on first cycle"
        );
    }

    // Clear slow device output to detect if it gets re-written
    {
        let mut state = slow_state.lock().unwrap();
        state.insert("DO_0".to_string(), st_comm_api::IoValue::Bool(false));
    }

    // Second cycle: slow device should NOT be written to (200ms not elapsed)
    engine.run_one_cycle().unwrap();
    {
        let state = slow_state.lock().unwrap();
        assert_eq!(
            state.get("DO_0"),
            Some(&st_comm_api::IoValue::Bool(false)),
            "Slow device DO_0 should NOT be rewritten (cycle_time not elapsed)"
        );
    }
}
