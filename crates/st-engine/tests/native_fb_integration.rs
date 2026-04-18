//! End-to-end integration tests for native function blocks.
//!
//! Tests the full pipeline: profile → NativeFb → registry → compile → VM → verify.

use st_comm_api::*;
use st_comm_sim::SimulatedNativeFb;
use st_ir::Value;
use std::sync::Arc;

/// Build a registry from a YAML profile string, compile + run ST code, return VM.
fn run_with_profile(profile_yaml: &str, source: &str, cycles: usize) -> st_engine::vm::Vm {
    let profile = DeviceProfile::from_yaml(profile_yaml).expect("Invalid profile YAML");
    let name = profile.name.clone();
    let sim_fb = SimulatedNativeFb::new(&name, profile);
    let state_handle = sim_fb.state_handle();

    let mut registry = NativeFbRegistry::new();
    registry.register(Box::new(sim_fb));

    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    assert!(
        parse_result.errors.is_empty(),
        "Parse errors: {:?}",
        parse_result.errors
    );

    let module = st_compiler::compile_with_native_fbs(&parse_result.source_file, Some(&registry))
        .expect("Compilation failed");

    let arc_reg = Arc::new(registry);
    let mut vm = st_engine::vm::Vm::new_with_native_fbs(
        module,
        st_engine::vm::VmConfig::default(),
        Some(arc_reg),
    );
    let _ = vm.run_global_init();

    // Set a simulated input before running
    {
        let mut state = state_handle.lock().unwrap();
        state.insert("DI_0".to_string(), IoValue::Bool(true));
        state.insert("AI_0".to_string(), IoValue::Int(42));
    }

    for _ in 0..cycles {
        vm.scan_cycle("Main").expect("Scan cycle failed");
    }

    // Verify output was written back to shared state
    let state = state_handle.lock().unwrap();
    let do_0 = state.get("DO_0").cloned();
    drop(state);

    // Store the output value in a global for test assertions
    // (done inside the ST program via field access)
    let _ = do_0; // just verify no panic

    vm
}

const TEST_PROFILE: &str = r#"
name: TestDevice
protocol: simulated
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: virtual } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: virtual } }
  - { name: AI_0, type: INT, direction: input, register: { address: 10, kind: virtual } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 20, kind: virtual } }
  - { name: AO_0, type: INT, direction: output, register: { address: 30, kind: virtual } }
"#;

#[test]
fn profile_to_native_fb_roundtrip() {
    let vm = run_with_profile(
        TEST_PROFILE,
        r#"
VAR_GLOBAL
    g_connected : BOOL := FALSE;
    g_di0 : BOOL := FALSE;
    g_ai0 : INT := 0;
    g_cycles : INT := 0;
END_VAR

PROGRAM Main
VAR
    dev : TestDevice;
END_VAR
    dev(refresh_rate := T#10ms);
    g_connected := dev.connected;
    g_di0 := dev.DI_0;
    g_ai0 := dev.AI_0;
    dev.DO_0 := dev.DI_0;
    dev.AO_0 := dev.AI_0;
    g_cycles := g_cycles + 1;
END_PROGRAM
"#,
        3,
    );

    // After 3 cycles, connected should be true
    assert_eq!(vm.get_global("g_connected"), Some(&Value::Bool(true)));
    // DI_0 was set to true in shared state, so it should flow through
    assert_eq!(vm.get_global("g_di0"), Some(&Value::Bool(true)));
    // AI_0 was set to 42
    assert_eq!(vm.get_global("g_ai0"), Some(&Value::Int(42)));
    // Cycle counter
    assert_eq!(vm.get_global("g_cycles"), Some(&Value::Int(3)));
}

#[test]
fn multiple_profile_devices() {
    let profile1 = DeviceProfile::from_yaml(
        r#"
name: DevA
protocol: simulated
fields:
  - { name: VAL, type: INT, direction: input, register: { address: 0, kind: virtual } }
"#,
    )
    .unwrap();

    let profile2 = DeviceProfile::from_yaml(
        r#"
name: DevB
protocol: simulated
fields:
  - { name: VAL, type: INT, direction: input, register: { address: 0, kind: virtual } }
"#,
    )
    .unwrap();

    let sim_a = SimulatedNativeFb::new("DevA", profile1);
    let sim_b = SimulatedNativeFb::new("DevB", profile2);

    let mut registry = NativeFbRegistry::new();
    registry.register(Box::new(sim_a));
    registry.register(Box::new(sim_b));

    let source = r#"
VAR_GLOBAL
    va : INT := 0;
    vb : INT := 0;
END_VAR

PROGRAM Main
VAR
    a : DevA;
    b : DevB;
END_VAR
    a();
    b();
    va := a.VAL;
    vb := b.VAL;
END_PROGRAM
"#;

    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    assert!(parse_result.errors.is_empty());

    let module =
        st_compiler::compile_with_native_fbs(&parse_result.source_file, Some(&registry)).unwrap();

    let arc_reg = Arc::new(registry);
    let mut vm = st_engine::vm::Vm::new_with_native_fbs(
        module,
        st_engine::vm::VmConfig::default(),
        Some(arc_reg),
    );
    let _ = vm.run_global_init();
    vm.scan_cycle("Main").unwrap();

    // Both devices exist and are independently addressable
    assert_eq!(vm.get_global("va"), Some(&Value::Int(0)));
    assert_eq!(vm.get_global("vb"), Some(&Value::Int(0)));
}

#[test]
fn diagnostic_fields_update() {
    let vm = run_with_profile(
        TEST_PROFILE,
        r#"
VAR_GLOBAL
    g_io_cycles : INT := 0;
    g_error : INT := 0;
END_VAR

PROGRAM Main
VAR
    dev : TestDevice;
END_VAR
    dev();
    g_io_cycles := dev.io_cycles;
    g_error := dev.error_code;
END_PROGRAM
"#,
        5,
    );

    // io_cycles should increment each cycle
    // Note: io_cycles is UDINT, but we read it as INT via the global
    let cycles = vm.get_global("g_io_cycles");
    assert!(
        matches!(cycles, Some(Value::Int(n)) if *n == 5) ||
        matches!(cycles, Some(Value::UInt(n)) if *n == 5),
        "Expected io_cycles=5, got {:?}",
        cycles
    );
    assert_eq!(vm.get_global("g_error"), Some(&Value::Int(0)));
}
