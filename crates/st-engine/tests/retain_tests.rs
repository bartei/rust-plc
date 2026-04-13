//! Tests for RETAIN / PERSISTENT variable persistence (Phase 16).

use st_engine::*;
use st_engine::retain_store::{self, RetainConfig, RetainSnapshot};
use st_ir::*;
use std::path::PathBuf;

/// Helper: parse + compile multi-file + run N cycles with retain config, return engine.
fn run_multi_with_retain(sources: &[&str], cycles: u64, retain_path: PathBuf) -> Engine {
    let parse_result = st_syntax::multi_file::parse_multi(sources);
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
        max_cycles: cycles,
        retain: Some(RetainConfig {
            path: retain_path,
            checkpoint_cycles: 0,
        }),
        ..Default::default()
    };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

/// Helper: parse + compile + run N cycles with retain config, return engine.
fn run_with_retain(source: &str, cycles: u64, retain_path: PathBuf) -> Engine {
    let parse_result = st_syntax::parse(source);
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
        max_cycles: cycles,
        retain: Some(RetainConfig {
            path: retain_path,
            checkpoint_cycles: 0,
        }),
        ..Default::default()
    };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

/// Helper: compile source into a module.
fn compile(source: &str) -> Module {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    st_compiler::compile(&parse_result.source_file).expect("Compile failed")
}

// =============================================================================
// Capture tests
// =============================================================================

#[test]
fn test_capture_empty_vm() {
    let source = r#"
VAR_GLOBAL g : INT; END_VAR
PROGRAM Main VAR x : INT; END_VAR x := x + 1; END_PROGRAM
"#;
    let engine = run_with_retain(source, 5, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());
    assert!(snapshot.globals.is_empty(), "No RETAIN globals should be captured");
    assert!(snapshot.program_locals.is_empty(), "No RETAIN locals should be captured");
}

#[test]
fn test_capture_retain_globals() {
    let source = r#"
VAR_GLOBAL RETAIN g_counter : INT; END_VAR
VAR_GLOBAL g_normal : INT; END_VAR
PROGRAM Main VAR END_VAR
    g_counter := g_counter + 1;
    g_normal := g_normal + 1;
END_PROGRAM
"#;
    let engine = run_with_retain(source, 10, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());
    assert!(snapshot.globals.contains_key("g_counter"), "RETAIN global should be captured");
    assert!(!snapshot.globals.contains_key("g_normal"), "Non-RETAIN global should NOT be captured");
    let entry = &snapshot.globals["g_counter"];
    assert!(entry.retain);
    assert!(!entry.persistent);
    assert_eq!(entry.value, Value::Int(10));
}

#[test]
fn test_capture_persistent_globals() {
    let source = r#"
VAR_GLOBAL PERSISTENT g_total : INT; END_VAR
PROGRAM Main VAR END_VAR
    g_total := g_total + 2;
END_PROGRAM
"#;
    let engine = run_with_retain(source, 5, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());
    assert!(snapshot.globals.contains_key("g_total"));
    let entry = &snapshot.globals["g_total"];
    assert!(!entry.retain);
    assert!(entry.persistent);
    assert_eq!(entry.value, Value::Int(10));
}

#[test]
fn test_capture_retain_persistent_globals() {
    let source = r#"
VAR_GLOBAL RETAIN PERSISTENT g_both : INT; END_VAR
PROGRAM Main VAR END_VAR
    g_both := g_both + 1;
END_PROGRAM
"#;
    let engine = run_with_retain(source, 3, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());
    let entry = &snapshot.globals["g_both"];
    assert!(entry.retain);
    assert!(entry.persistent);
    assert_eq!(entry.value, Value::Int(3));
}

#[test]
fn test_non_retain_globals_excluded() {
    let source = r#"
VAR_GLOBAL a : INT; b : BOOL; c : REAL; END_VAR
PROGRAM Main VAR END_VAR a := 42; b := TRUE; c := 1.23; END_PROGRAM
"#;
    let engine = run_with_retain(source, 1, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());
    assert!(snapshot.globals.is_empty(), "Plain globals should not be captured");
}

#[test]
fn test_capture_retain_locals() {
    let source = r#"
PROGRAM Main
VAR RETAIN counter : INT; END_VAR
VAR normal : INT; END_VAR
    counter := counter + 1;
    normal := normal + 1;
END_PROGRAM
"#;
    let engine = run_with_retain(source, 10, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());
    assert!(snapshot.program_locals.contains_key("Main"), "Main should have retain locals");
    let main_locals = &snapshot.program_locals["Main"];
    assert!(main_locals.contains_key("counter"), "RETAIN local should be captured");
    assert!(!main_locals.contains_key("normal"), "Non-RETAIN local should NOT be captured");
    assert_eq!(main_locals["counter"].value, Value::Int(10));
}

// =============================================================================
// Restore tests
// =============================================================================

#[test]
fn test_restore_globals_warm() {
    let source = r#"
VAR_GLOBAL RETAIN g_counter : INT; END_VAR
PROGRAM Main VAR END_VAR g_counter := g_counter + 1; END_PROGRAM
"#;
    let module = compile(source);
    let mut vm = Vm::new(module.clone(), VmConfig::default());
    let _ = vm.run_global_init();

    // Build a snapshot with a saved value
    let mut snapshot = RetainSnapshot {
        version: 1,
        created_at: 0,
        globals: std::collections::HashMap::new(),
        program_locals: std::collections::HashMap::new(),
        instance_fields: std::collections::HashMap::new(),
    };
    snapshot.globals.insert("g_counter".to_string(), retain_store::RetainEntry {
        value: Value::Int(42),
        retain: true,
        persistent: false,
    });

    let warnings = retain_store::restore_snapshot(&mut vm, &snapshot, true);
    assert!(warnings.is_empty(), "No warnings expected: {warnings:?}");
    assert_eq!(vm.get_global("g_counter"), Some(&Value::Int(42)));
}

#[test]
fn test_restore_globals_cold() {
    let source = r#"
VAR_GLOBAL PERSISTENT g_total : INT; END_VAR
PROGRAM Main VAR END_VAR g_total := g_total + 1; END_PROGRAM
"#;
    let module = compile(source);
    let mut vm = Vm::new(module, VmConfig::default());
    let _ = vm.run_global_init();

    let mut snapshot = RetainSnapshot {
        version: 1, created_at: 0,
        globals: std::collections::HashMap::new(),
        program_locals: std::collections::HashMap::new(),
        instance_fields: std::collections::HashMap::new(),
    };
    snapshot.globals.insert("g_total".to_string(), retain_store::RetainEntry {
        value: Value::Int(99),
        retain: false,
        persistent: true,
    });

    let warnings = retain_store::restore_snapshot(&mut vm, &snapshot, false);
    assert!(warnings.is_empty());
    assert_eq!(vm.get_global("g_total"), Some(&Value::Int(99)));
}

#[test]
fn test_retain_not_restored_on_cold() {
    let source = r#"
VAR_GLOBAL RETAIN g_r : INT; END_VAR
PROGRAM Main VAR END_VAR g_r := 1; END_PROGRAM
"#;
    let module = compile(source);
    let mut vm = Vm::new(module, VmConfig::default());
    let _ = vm.run_global_init();

    let mut snapshot = RetainSnapshot {
        version: 1, created_at: 0,
        globals: std::collections::HashMap::new(),
        program_locals: std::collections::HashMap::new(),
        instance_fields: std::collections::HashMap::new(),
    };
    snapshot.globals.insert("g_r".to_string(), retain_store::RetainEntry {
        value: Value::Int(50),
        retain: true,
        persistent: false,
    });

    // Cold restart: RETAIN-only should NOT be restored
    let warnings = retain_store::restore_snapshot(&mut vm, &snapshot, false);
    assert!(warnings.is_empty());
    assert_eq!(vm.get_global("g_r"), Some(&Value::Int(0)), "RETAIN should be cleared on cold restart");
}

#[test]
fn test_persistent_not_restored_on_warm() {
    let source = r#"
VAR_GLOBAL PERSISTENT g_p : INT; END_VAR
PROGRAM Main VAR END_VAR g_p := 1; END_PROGRAM
"#;
    let module = compile(source);
    let mut vm = Vm::new(module, VmConfig::default());
    let _ = vm.run_global_init();

    let mut snapshot = RetainSnapshot {
        version: 1, created_at: 0,
        globals: std::collections::HashMap::new(),
        program_locals: std::collections::HashMap::new(),
        instance_fields: std::collections::HashMap::new(),
    };
    snapshot.globals.insert("g_p".to_string(), retain_store::RetainEntry {
        value: Value::Int(50),
        retain: false,
        persistent: true,
    });

    // Warm restart: PERSISTENT-only should NOT be restored
    let warnings = retain_store::restore_snapshot(&mut vm, &snapshot, true);
    assert!(warnings.is_empty());
    assert_eq!(vm.get_global("g_p"), Some(&Value::Int(0)), "PERSISTENT should be cleared on warm restart");
}

#[test]
fn test_restore_type_mismatch() {
    let source = r#"
VAR_GLOBAL RETAIN g_val : BOOL; END_VAR
PROGRAM Main VAR END_VAR g_val := TRUE; END_PROGRAM
"#;
    let module = compile(source);
    let mut vm = Vm::new(module, VmConfig::default());
    let _ = vm.run_global_init();

    let mut snapshot = RetainSnapshot {
        version: 1, created_at: 0,
        globals: std::collections::HashMap::new(),
        program_locals: std::collections::HashMap::new(),
        instance_fields: std::collections::HashMap::new(),
    };
    // Wrong type: INT value for BOOL slot
    snapshot.globals.insert("g_val".to_string(), retain_store::RetainEntry {
        value: Value::Int(42),
        retain: true,
        persistent: false,
    });

    let warnings = retain_store::restore_snapshot(&mut vm, &snapshot, true);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("type mismatch"));
}

#[test]
fn test_restore_missing_variable() {
    let source = r#"
VAR_GLOBAL RETAIN g_exists : INT; END_VAR
PROGRAM Main VAR END_VAR g_exists := 1; END_PROGRAM
"#;
    let module = compile(source);
    let mut vm = Vm::new(module, VmConfig::default());
    let _ = vm.run_global_init();

    let mut snapshot = RetainSnapshot {
        version: 1, created_at: 0,
        globals: std::collections::HashMap::new(),
        program_locals: std::collections::HashMap::new(),
        instance_fields: std::collections::HashMap::new(),
    };
    snapshot.globals.insert("g_deleted".to_string(), retain_store::RetainEntry {
        value: Value::Int(99),
        retain: true,
        persistent: false,
    });

    let warnings = retain_store::restore_snapshot(&mut vm, &snapshot, true);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("no longer exists"));
}

// =============================================================================
// File round-trip
// =============================================================================

#[test]
fn test_file_round_trip() {
    let source = r#"
VAR_GLOBAL RETAIN g_a : INT; END_VAR
VAR_GLOBAL RETAIN g_b : BOOL; END_VAR
VAR_GLOBAL PERSISTENT g_c : REAL; END_VAR
PROGRAM Main VAR END_VAR
    g_a := 42;
    g_b := TRUE;
    g_c := 1.23;
END_PROGRAM
"#;
    let engine = run_with_retain(source, 1, PathBuf::from("/tmp/no-write.retain"));
    let snapshot = retain_store::capture_snapshot(engine.vm());

    let tmp = std::env::temp_dir().join("st_retain_test_roundtrip.retain");
    retain_store::save_to_file(&snapshot, &tmp).expect("Save failed");

    let loaded = retain_store::load_from_file(&tmp).expect("Load failed");
    assert_eq!(loaded.globals.len(), snapshot.globals.len());
    assert_eq!(loaded.globals["g_a"].value, Value::Int(42));
    assert_eq!(loaded.globals["g_b"].value, Value::Bool(true));
    // REAL comparison with tolerance
    if let Value::Real(r) = loaded.globals["g_c"].value {
        assert!((r - 1.23).abs() < 0.001);
    } else {
        panic!("Expected REAL");
    }

    // Clean up
    let _ = std::fs::remove_file(&tmp);
}

// =============================================================================
// Engine restart integration
// =============================================================================

#[test]
fn test_retain_across_engine_restart() {
    let source = r#"
VAR_GLOBAL RETAIN g_counter : INT; END_VAR
PROGRAM Main VAR END_VAR
    g_counter := g_counter + 1;
END_PROGRAM
"#;
    let tmp = std::env::temp_dir().join("st_retain_test_restart.retain");
    let _ = std::fs::remove_file(&tmp); // ensure clean start

    // First run: 10 cycles
    {
        let engine = run_with_retain(source, 10, tmp.clone());
        assert_eq!(engine.vm().get_global("g_counter"), Some(&Value::Int(10)));
        engine.save_retain().expect("Save failed");
    }

    // Second run: engine should restore from file, then run 5 more cycles
    {
        let engine = run_with_retain(source, 5, tmp.clone());
        // Should be 10 (restored) + 5 (new cycles) = 15
        assert_eq!(
            engine.vm().get_global("g_counter"),
            Some(&Value::Int(15)),
            "Counter should continue from restored value"
        );
    }

    // Clean up
    let _ = std::fs::remove_file(&tmp);
}

// =============================================================================
// Struct PERSISTENT RETAIN — the original bug
// =============================================================================

#[test]
fn test_struct_persistent_retain_captured() {
    let type_src = r#"
TYPE
    ProcessData : STRUCT
        bottles_filled : INT := 0;
        running : BOOL := FALSE;
    END_STRUCT;
END_TYPE
"#;
    let main_src = r#"
PROGRAM Main
VAR PERSISTENT RETAIN
    stats : ProcessData;
END_VAR
    stats.bottles_filled := stats.bottles_filled + 1;
    stats.running := TRUE;
END_PROGRAM
"#;
    let tmp = std::env::temp_dir().join("st_retain_test_struct_capture.retain");
    let _ = std::fs::remove_file(&tmp);

    let engine = run_multi_with_retain(&[type_src, main_src], 10, tmp.clone());
    let snapshot = retain_store::capture_snapshot(engine.vm());

    // The struct fields should appear in instance_fields
    assert!(
        snapshot.instance_fields.contains_key("Main"),
        "Main program should have instance_fields"
    );
    let main_instances = &snapshot.instance_fields["Main"];
    assert!(
        main_instances.contains_key("stats"),
        "stats struct should be captured"
    );
    let stats_fields = &main_instances["stats"];
    assert_eq!(
        stats_fields["bottles_filled"].value,
        Value::Int(10),
        "bottles_filled should be 10 after 10 cycles"
    );
    assert_eq!(
        stats_fields["running"].value,
        Value::Bool(true),
        "running should be TRUE"
    );
    assert!(stats_fields["bottles_filled"].retain);
    assert!(stats_fields["bottles_filled"].persistent);

    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_struct_persistent_retain_survives_restart() {
    // Mirrors the real main.st: non-retain locals with initial values
    // coexist with a PERSISTENT RETAIN struct. The non-retain locals
    // MUST get their declared initial values on restart — if they don't
    // (e.g. `moving` stays FALSE), the program logic breaks.
    let type_src = r#"
TYPE
    ProcessData : STRUCT
        bottles_filled : INT := 0;
        running : BOOL := FALSE;
    END_STRUCT;
END_TYPE
"#;
    let main_src = r#"
PROGRAM Main
VAR
    cycle : INT := 0;
    moving : BOOL := TRUE;
END_VAR
VAR PERSISTENT RETAIN
    stats : ProcessData;
END_VAR
    cycle := cycle + 1;
    stats.bottles_filled := stats.bottles_filled + 1;
    stats.running := moving;
END_PROGRAM
"#;
    let tmp = std::env::temp_dir().join("st_retain_test_struct_restart.retain");
    let _ = std::fs::remove_file(&tmp);

    // First run: 10 cycles → bottles_filled = 10, moving = TRUE
    {
        let engine = run_multi_with_retain(&[type_src, main_src], 10, tmp.clone());
        engine.save_retain().expect("Save failed");

        let vars = engine.vm().monitorable_variables();
        let bf = vars.iter().find(|v| v.name == "Main.stats.bottles_filled");
        assert!(bf.is_some(), "bottles_filled should be monitorable");
        assert_eq!(bf.unwrap().value, "10");
    }

    // Second run: restore + 5 more cycles → bottles_filled = 15
    {
        let engine = run_multi_with_retain(&[type_src, main_src], 5, tmp.clone());
        let vars = engine.vm().monitorable_variables();

        // Struct PERSISTENT RETAIN field must continue from restored value
        let bf = vars
            .iter()
            .find(|v| v.name == "Main.stats.bottles_filled")
            .expect("bottles_filled should exist after restart");
        assert_eq!(
            bf.value, "15",
            "bottles_filled should be 10 (restored) + 5 (new cycles) = 15"
        );

        // Non-retain local with initial value MUST get its declared default
        let running = vars
            .iter()
            .find(|v| v.name == "Main.stats.running")
            .expect("running should exist");
        assert_eq!(
            running.value, "TRUE",
            "stats.running must be TRUE — moving := TRUE is the init value"
        );

        // Non-retain counter must start fresh (not carry over from prior run)
        let cycle = vars
            .iter()
            .find(|v| v.name == "Main.cycle")
            .expect("cycle should exist");
        assert_eq!(
            cycle.value, "5",
            "cycle must be 5 (fresh start, not 15) — non-retain locals reset on restart"
        );
    }

    let _ = std::fs::remove_file(&tmp);
}
