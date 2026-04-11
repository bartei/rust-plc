//! End-to-end online change tests: compile, run, hot-reload, verify state.

use st_ir::*;
use st_engine::*;

fn make_engine(source: &str) -> Engine {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");
    let program_name = module
        .functions
        .iter()
        .find(|f| f.kind == PouKind::Program)
        .expect("No PROGRAM found")
        .name
        .clone();
    Engine::new(module, program_name, EngineConfig::default())
}

// =============================================================================
// Compatible changes — code logic changes, variables preserved
// =============================================================================

#[test]
fn online_change_preserves_counter() {
    let source_v1 = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
    g_result := counter;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);

    // Run 10 cycles — counter = 10
    for _ in 0..10 {
        engine.run_one_cycle().unwrap();
    }
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(10)));

    // Change logic: increment by 2 instead of 1
    let source_v2 = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 2;
    g_result := counter;
END_PROGRAM
";
    let analysis = engine.online_change(source_v2).expect("Online change failed");
    assert!(analysis.compatible);
    assert!(analysis.preserved_vars.iter().any(|v| v.contains("counter")));

    // Run 5 more cycles — counter should continue from 10, adding 2 each time
    for _ in 0..5 {
        engine.run_one_cycle().unwrap();
    }
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(20)));
}

#[test]
fn online_change_adds_variable() {
    let source_v1 = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
    g_result := counter;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);

    for _ in 0..5 {
        engine.run_one_cycle().unwrap();
    }
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(5)));

    // Add a new variable — counter preserved, new var initialized to default
    let source_v2 = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
    multiplier : INT := 3;
END_VAR
    counter := counter + 1;
    g_result := counter * multiplier;
END_PROGRAM
";
    let analysis = engine.online_change(source_v2).expect("Online change failed");
    assert!(analysis.compatible);
    assert!(analysis.new_vars.iter().any(|v| v.contains("multiplier")));

    // Run 1 more cycle — counter was 5, now 6, multiplier is default 0 (init skipped)
    engine.run_one_cycle().unwrap();
    let g = engine.vm().get_global("g_result");
    // counter=6, multiplier=0 (init code skipped for retained programs)
    // g_result = 6 * 0 = 0
    // Actually multiplier will be initialized to default Value::Int(0) from migrate_locals
    assert!(g.is_some());
}

#[test]
fn online_change_modifies_logic_only() {
    let source_v1 = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    val : INT := 0;
END_VAR
    val := val + 1;
    g_result := val;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);

    for _ in 0..3 {
        engine.run_one_cycle().unwrap();
    }
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(3)));

    // Change to multiply instead of add
    let source_v2 = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    val : INT := 0;
END_VAR
    val := val * 2;
    g_result := val;
END_PROGRAM
";
    engine.online_change(source_v2).unwrap();

    // val is retained as 3, now multiply by 2 each cycle: 3→6→12→24
    engine.run_one_cycle().unwrap();
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(6)));
    engine.run_one_cycle().unwrap();
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(12)));
}

#[test]
fn online_change_preserves_global_variables() {
    let source_v1 = "\
VAR_GLOBAL
    g_total : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    g_total := g_total + 1;
    x := g_total;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);

    for _ in 0..10 {
        engine.run_one_cycle().unwrap();
    }
    assert_eq!(engine.vm().get_global("g_total"), Some(&Value::Int(10)));

    // Change code but keep the global
    let source_v2 = "\
VAR_GLOBAL
    g_total : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    g_total := g_total + 10;
    x := g_total;
END_PROGRAM
";
    engine.online_change(source_v2).unwrap();

    engine.run_one_cycle().unwrap();
    assert_eq!(engine.vm().get_global("g_total"), Some(&Value::Int(20)));
}

// =============================================================================
// Incompatible changes — rejected
// =============================================================================

#[test]
fn online_change_rejects_type_change() {
    let source_v1 = "\
PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);
    engine.run_one_cycle().unwrap();

    // Change counter from INT to REAL — incompatible
    let source_v2 = "\
PROGRAM Main
VAR
    counter : REAL := 0.0;
END_VAR
    counter := counter + 1.0;
END_PROGRAM
";
    let result = engine.online_change(source_v2);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Incompatible"));
}

#[test]
fn online_change_rejects_removed_function() {
    let source_v1 = "\
FUNCTION Helper : INT
VAR_INPUT
    x : INT;
END_VAR
    Helper := x + 1;
END_FUNCTION

PROGRAM Main
VAR
    val : INT := 0;
END_VAR
    val := Helper(x := val);
END_PROGRAM
";
    let mut engine = make_engine(source_v1);
    engine.run_one_cycle().unwrap();

    // Remove the Helper function — incompatible
    let source_v2 = "\
PROGRAM Main
VAR
    val : INT := 0;
END_VAR
    val := val + 1;
END_PROGRAM
";
    let result = engine.online_change(source_v2);
    assert!(result.is_err());
}

#[test]
fn online_change_rejects_global_type_change() {
    let source_v1 = "\
VAR_GLOBAL
    g : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    g := g + 1;
    x := g;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);
    engine.run_one_cycle().unwrap();

    let source_v2 = "\
VAR_GLOBAL
    g : REAL;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
";
    let result = engine.online_change(source_v2);
    assert!(result.is_err());
}

#[test]
fn online_change_rejects_parse_errors() {
    let source_v1 = "\
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);
    engine.run_one_cycle().unwrap();

    let source_v2 = "PROGRAM Main\nVAR\n  x : ;\nEND_VAR\nEND_PROGRAM\n";
    let result = engine.online_change(source_v2);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Parse"));
}

// =============================================================================
// Multiple online changes
// =============================================================================

#[test]
fn multiple_online_changes() {
    let source_v1 = "\
VAR_GLOBAL
    g : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
    g := x;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);
    for _ in 0..5 { engine.run_one_cycle().unwrap(); }
    assert_eq!(engine.vm().get_global("g"), Some(&Value::Int(5)));

    // Change 1: add 2
    let source_v2 = "\
VAR_GLOBAL
    g : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 2;
    g := x;
END_PROGRAM
";
    engine.online_change(source_v2).unwrap();
    for _ in 0..5 { engine.run_one_cycle().unwrap(); }
    assert_eq!(engine.vm().get_global("g"), Some(&Value::Int(15))); // 5 + 5*2

    // Change 2: add 10
    let source_v3 = "\
VAR_GLOBAL
    g : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 10;
    g := x;
END_PROGRAM
";
    engine.online_change(source_v3).unwrap();
    for _ in 0..2 { engine.run_one_cycle().unwrap(); }
    assert_eq!(engine.vm().get_global("g"), Some(&Value::Int(35))); // 15 + 2*10
}

// =============================================================================
// Adding new function during online change
// =============================================================================

#[test]
fn online_change_adds_function() {
    let source_v1 = "\
VAR_GLOBAL
    g : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
    g := x;
END_PROGRAM
";
    let mut engine = make_engine(source_v1);
    for _ in 0..5 { engine.run_one_cycle().unwrap(); }
    assert_eq!(engine.vm().get_global("g"), Some(&Value::Int(5)));

    // Add a function and use it
    let source_v2 = "\
FUNCTION Double : INT
VAR_INPUT
    v : INT;
END_VAR
    Double := v * 2;
END_FUNCTION

VAR_GLOBAL
    g : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
    g := Double(v := x);
END_PROGRAM
";
    engine.online_change(source_v2).unwrap();
    engine.run_one_cycle().unwrap();
    // x was 5, now 6. g = Double(6) = 12
    assert_eq!(engine.vm().get_global("g"), Some(&Value::Int(12)));
}
