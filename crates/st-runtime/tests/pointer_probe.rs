//! Pointer probing tests: verify actual runtime values for every pointer scenario.
//! These tests exist to find bugs BEFORE writing playground examples.

use st_ir::*;
use st_runtime::*;

fn run_program(source: &str, cycles: u64) -> Engine {
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
        ..Default::default()
    };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

fn run_function(source: &str, func_name: &str) -> Value {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let mut vm = Vm::new(module, VmConfig::default());
    vm.run(func_name).unwrap()
}

// =============================================================================
// 1. Basic REF/deref round-trip (sanity check)
// =============================================================================

#[test]
fn ref_deref_round_trip_int() {
    let val = run_function(r#"
FUNCTION Test : INT
VAR_INPUT dummy : INT; END_VAR
VAR
    x : INT := 77;
    p : REF_TO INT;
END_VAR
    p := REF(x);
    Test := p^;
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Int(77));
}

#[test]
fn ref_deref_round_trip_real() {
    let val = run_function(r#"
FUNCTION Test : REAL
VAR_INPUT dummy : INT; END_VAR
VAR
    x : REAL := 3.125;
    p : REF_TO REAL;
END_VAR
    p := REF(x);
    Test := p^;
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Real(3.125));
}

#[test]
fn ref_deref_round_trip_bool() {
    let val = run_function(r#"
FUNCTION Test : INT
VAR_INPUT dummy : INT; END_VAR
VAR
    flag : BOOL := TRUE;
    p : REF_TO BOOL;
END_VAR
    p := REF(flag);
    IF p^ THEN
        Test := 1;
    ELSE
        Test := 0;
    END_IF;
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Int(1));
}

// =============================================================================
// 2. Write through pointer modifies original
// =============================================================================

#[test]
fn write_through_pointer_modifies_local() {
    let source = r#"
VAR_GLOBAL g_x : INT; g_y : INT; END_VAR
PROGRAM Main
VAR
    x : INT := 10;
    y : INT := 20;
    p : REF_TO INT;
END_VAR
    p := REF(x);
    p^ := 100;
    g_x := x;

    p := REF(y);
    p^ := 200;
    g_y := y;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_x"), Some(&Value::Int(100)));
    assert_eq!(engine.vm().get_global("g_y"), Some(&Value::Int(200)));
}

#[test]
fn write_through_pointer_to_global() {
    let source = r#"
VAR_GLOBAL
    g_target : INT;
    g_check  : INT;
END_VAR
PROGRAM Main
VAR
    p : REF_TO INT;
END_VAR
    g_target := 0;
    p := REF(g_target);
    p^ := 999;
    g_check := g_target;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_target"), Some(&Value::Int(999)));
    assert_eq!(engine.vm().get_global("g_check"), Some(&Value::Int(999)));
}

// =============================================================================
// 3. Pointer reassignment — point to different variables
// =============================================================================

#[test]
fn pointer_reassignment() {
    let source = r#"
VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    a : INT := 10;
    b : INT := 20;
    c : INT := 30;
    p : REF_TO INT;
END_VAR
    // Point to a, read it
    p := REF(a);
    g_result := p^;      // expect 10
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(10)));
}

#[test]
fn pointer_reassignment_sequence() {
    let source = r#"
VAR_GLOBAL g_r1 : INT; g_r2 : INT; g_r3 : INT; END_VAR
PROGRAM Main
VAR
    a : INT := 10;
    b : INT := 20;
    c : INT := 30;
    p : REF_TO INT;
END_VAR
    p := REF(a);
    g_r1 := p^;

    p := REF(b);
    g_r2 := p^;

    p := REF(c);
    g_r3 := p^;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_r1"), Some(&Value::Int(10)));
    assert_eq!(engine.vm().get_global("g_r2"), Some(&Value::Int(20)));
    assert_eq!(engine.vm().get_global("g_r3"), Some(&Value::Int(30)));
}

// =============================================================================
// 4. NULL pointer safety
// =============================================================================

#[test]
fn null_deref_returns_zero() {
    let source = r#"
VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    p : REF_TO INT;
END_VAR
    g_result := p^;    // uninitialized = NULL, deref returns 0
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(0)));
}

#[test]
fn null_write_is_noop() {
    let source = r#"
VAR_GLOBAL g_ok : INT; END_VAR
PROGRAM Main
VAR
    p : REF_TO INT;
END_VAR
    p^ := 999;        // write to NULL pointer — should be silent no-op
    g_ok := 1;         // we should reach here (no crash)
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_ok"), Some(&Value::Int(1)));
}

#[test]
fn assign_null_clears_pointer() {
    let source = r#"
VAR_GLOBAL g_before : INT; g_after : INT; END_VAR
PROGRAM Main
VAR
    x : INT := 42;
    p : REF_TO INT;
END_VAR
    p := REF(x);
    g_before := p^;       // 42
    p := NULL;
    g_after := p^;        // 0 (NULL deref)
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_before"), Some(&Value::Int(42)));
    assert_eq!(engine.vm().get_global("g_after"), Some(&Value::Int(0)));
}

// =============================================================================
// 5. Pointer passed to function (by value — the pointer itself is copied)
// =============================================================================

#[test]
fn pass_pointer_to_function_read() {
    let source = r#"
FUNCTION ReadVia : INT
VAR_INPUT
    p : REF_TO INT;
END_VAR
    ReadVia := p^;
END_FUNCTION

VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    x : INT := 55;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(x);
    g_result := ReadVia(p := ptr);
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(55)));
}

#[test]
fn pass_pointer_to_function_write() {
    let source = r#"
FUNCTION WriteVia : INT
VAR_INPUT
    p : REF_TO INT;
    val : INT;
END_VAR
    p^ := val;
    WriteVia := 0;
END_FUNCTION

VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    x : INT := 0;
    ptr : REF_TO INT;
    dummy : INT;
END_VAR
    ptr := REF(x);
    dummy := WriteVia(p := ptr, val := 88);
    g_result := x;    // should be 88 after function wrote through pointer
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(88)));
}

// =============================================================================
// 6. Pointer arithmetic via read-modify-write
// =============================================================================

#[test]
fn increment_via_pointer() {
    let source = r#"
FUNCTION Inc : INT
VAR_INPUT
    p : REF_TO INT;
END_VAR
    p^ := p^ + 1;
    Inc := p^;
END_FUNCTION

VAR_GLOBAL g_val : INT; g_ret : INT; END_VAR
PROGRAM Main
VAR
    counter : INT := 0;
    ptr : REF_TO INT;
    dummy : INT;
END_VAR
    ptr := REF(counter);
    g_ret := Inc(p := ptr);
    g_val := counter;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(1)));
    assert_eq!(engine.vm().get_global("g_ret"), Some(&Value::Int(1)));
}

// =============================================================================
// 7. Multi-cycle pointer behavior — pointer to program local persists
// =============================================================================

#[test]
fn pointer_persists_across_cycles() {
    let source = r#"
VAR_GLOBAL g_count : INT; END_VAR
PROGRAM Main
VAR
    counter : INT := 0;
    ptr : REF_TO INT;
    initialized : BOOL := FALSE;
END_VAR
    IF NOT initialized THEN
        ptr := REF(counter);
        initialized := TRUE;
    END_IF;
    ptr^ := ptr^ + 1;
    g_count := counter;
END_PROGRAM
"#;
    let engine = run_program(source, 10);
    assert_eq!(engine.vm().get_global("g_count"), Some(&Value::Int(10)));
}

// =============================================================================
// 8. Pointer to global, modified from program across cycles
// =============================================================================

#[test]
fn pointer_to_global_across_cycles() {
    let source = r#"
VAR_GLOBAL
    g_accumulator : INT;
END_VAR
PROGRAM Main
VAR
    p : REF_TO INT;
END_VAR
    p := REF(g_accumulator);
    p^ := p^ + 5;
END_PROGRAM
"#;
    let engine = run_program(source, 4);
    assert_eq!(engine.vm().get_global("g_accumulator"), Some(&Value::Int(20))); // 5*4
}

// =============================================================================
// 9. Swap two values using pointers (classic pattern)
// =============================================================================

#[test]
fn swap_via_pointers() {
    let source = r#"
FUNCTION Swap : INT
VAR_INPUT
    p1 : REF_TO INT;
    p2 : REF_TO INT;
END_VAR
VAR
    temp : INT;
END_VAR
    temp := p1^;
    p1^ := p2^;
    p2^ := temp;
    Swap := 0;
END_FUNCTION

VAR_GLOBAL g_a : INT; g_b : INT; END_VAR
PROGRAM Main
VAR
    a : INT := 100;
    b : INT := 200;
    dummy : INT;
END_VAR
    dummy := Swap(p1 := REF(a), p2 := REF(b));
    g_a := a;
    g_b := b;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_a"), Some(&Value::Int(200)));
    assert_eq!(engine.vm().get_global("g_b"), Some(&Value::Int(100)));
}

// =============================================================================
// 10. Conditional pointer target
// =============================================================================

#[test]
fn conditional_pointer_target() {
    let source = r#"
VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    a : INT := 10;
    b : INT := 20;
    p : REF_TO INT;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    IF (cycle MOD 2) = 1 THEN
        p := REF(a);
    ELSE
        p := REF(b);
    END_IF;
    p^ := p^ + 1;
    g_result := a + b;
END_PROGRAM
"#;
    // After 6 cycles: odd cycles (1,3,5) increment a, even (2,4,6) increment b
    // a = 10 + 3 = 13, b = 20 + 3 = 23, result = 36
    let engine = run_program(source, 6);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(36)));
}

// =============================================================================
// 11. Pointer in a loop — scan array via pointer
// =============================================================================

#[test]
fn pointer_in_loop_accumulate() {
    let source = r#"
VAR_GLOBAL g_sum : INT; END_VAR
PROGRAM Main
VAR
    v1 : INT := 10;
    v2 : INT := 20;
    v3 : INT := 30;
    v4 : INT := 40;
    p : REF_TO INT;
    sum : INT := 0;
END_VAR
    p := REF(v1); sum := sum + p^;
    p := REF(v2); sum := sum + p^;
    p := REF(v3); sum := sum + p^;
    p := REF(v4); sum := sum + p^;
    g_sum := sum;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_sum"), Some(&Value::Int(100)));
}

// =============================================================================
// 12. Two pointers to the same variable
// =============================================================================

#[test]
fn two_pointers_same_variable() {
    let source = r#"
VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    x : INT := 0;
    p1 : REF_TO INT;
    p2 : REF_TO INT;
END_VAR
    p1 := REF(x);
    p2 := REF(x);
    p1^ := 50;
    g_result := p2^;     // should see 50 (both point to x)
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(50)));
}

// =============================================================================
// 13. Pointer to REAL — write and read
// =============================================================================

#[test]
fn pointer_to_real() {
    let source = r#"
FUNCTION Test : REAL
VAR_INPUT dummy : INT; END_VAR
VAR
    x : REAL := 0.0;
    p : REF_TO REAL;
END_VAR
    p := REF(x);
    p^ := 2.875;
    Test := x;
END_FUNCTION
"#;
    let val = run_function(source, "Test");
    assert_eq!(val, Value::Real(2.875));
}

// =============================================================================
// 14. Function that finds max via pointers (returns pointer-like behavior)
// =============================================================================

#[test]
fn find_and_modify_max() {
    let source = r#"
FUNCTION SelectAndDouble : INT
VAR_INPUT
    pa : REF_TO INT;
    pb : REF_TO INT;
END_VAR
    IF pa^ > pb^ THEN
        pa^ := pa^ * 2;
        SelectAndDouble := pa^;
    ELSE
        pb^ := pb^ * 2;
        SelectAndDouble := pb^;
    END_IF;
END_FUNCTION

VAR_GLOBAL g_a : INT; g_b : INT; g_ret : INT; END_VAR
PROGRAM Main
VAR
    a : INT := 30;
    b : INT := 50;
END_VAR
    g_ret := SelectAndDouble(pa := REF(a), pb := REF(b));
    g_a := a;
    g_b := b;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_a"), Some(&Value::Int(30)));   // unchanged
    assert_eq!(engine.vm().get_global("g_b"), Some(&Value::Int(100)));  // 50*2
    assert_eq!(engine.vm().get_global("g_ret"), Some(&Value::Int(100)));
}

// =============================================================================
// 15. Pointer in FB-like pattern (class method takes pointer)
// =============================================================================

#[test]
fn class_method_takes_pointer() {
    let source = r#"
CLASS Doubler
METHOD Apply : INT
VAR_INPUT
    target : REF_TO INT;
END_VAR
    target^ := target^ * 2;
    Apply := target^;
END_METHOD
END_CLASS

VAR_GLOBAL g_result : INT; g_val : INT; END_VAR
PROGRAM Main
VAR
    d : Doubler;
    x : INT := 21;
END_VAR
    g_result := d.Apply(target := REF(x));
    g_val := x;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(42)));
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(42)));
}

// =============================================================================
// 16. FB with internal pointer state
// =============================================================================

#[test]
fn fb_with_pointer_input() {
    let source = r#"
FUNCTION_BLOCK Incrementer
VAR_INPUT
    target : REF_TO INT;
    amount : INT;
END_VAR
    IF target <> NULL THEN
        target^ := target^ + amount;
    END_IF;
END_FUNCTION_BLOCK

VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    inc : Incrementer;
    counter : INT := 0;
END_VAR
    inc(target := REF(counter), amount := 3);
    g_val := counter;
END_PROGRAM
"#;
    let engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(15))); // 3*5
}
