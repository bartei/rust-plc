//! Probing tests: struct + pointer interactions.
//! Goal: map exactly what works vs what's broken before writing playground.

use st_ir::*;
use st_engine::*;

fn run_program(source: &str, cycles: u64) -> Engine {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");
    let program_name = module.functions.iter()
        .find(|f| f.kind == PouKind::Program).expect("No PROGRAM").name.clone();
    let config = EngineConfig { max_cycles: cycles, ..Default::default() };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

// =============================================================================
// 1. FB field READ — does LoadField work?
// =============================================================================

#[test]
fn fb_field_read_output() {
    let source = r#"
FUNCTION_BLOCK Counter
VAR_INPUT
    reset : BOOL;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
VAR
    internal : INT := 0;
END_VAR
    IF reset THEN
        internal := 0;
    ELSE
        internal := internal + 1;
    END_IF;
    count := internal;
END_FUNCTION_BLOCK

VAR_GLOBAL g_count : INT; END_VAR
PROGRAM Main
VAR
    c : Counter;
END_VAR
    c(reset := FALSE);
    g_count := c.count;
END_PROGRAM
"#;
    let engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_count"), Some(&Value::Int(5)));
}

// =============================================================================
// 2. FB field WRITE — does StoreField work?
// =============================================================================

#[test]
fn fb_field_write_input() {
    // Write to an FB's VAR_INPUT field before calling it
    let source = r#"
FUNCTION_BLOCK Adder
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
VAR_OUTPUT
    result : INT;
END_VAR
    result := a + b;
END_FUNCTION_BLOCK

VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR
    add : Adder;
END_VAR
    add(a := 10, b := 20);
    g_result := add.result;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(30)));
}

// =============================================================================
// 3. Direct FB field assignment (not via call syntax)
// =============================================================================

#[test]
fn fb_field_direct_assign() {
    // Assign to fb.field directly (not via call parameters)
    let source = r#"
FUNCTION_BLOCK Holder
VAR_INPUT
    value : INT;
END_VAR
VAR_OUTPUT
    doubled : INT;
END_VAR
    doubled := value * 2;
END_FUNCTION_BLOCK

VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    h : Holder;
END_VAR
    h.value := 21;
    h();
    g_val := h.doubled;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(42)));
}

// =============================================================================
// 4. Class field read via method (already proven to work)
// =============================================================================

#[test]
fn class_field_read_via_method() {
    let source = r#"
CLASS Box
VAR
    _width : INT := 0;
    _height : INT := 0;
END_VAR
METHOD SetSize
VAR_INPUT w : INT; h : INT; END_VAR
    _width := w;
    _height := h;
END_METHOD
METHOD Area : INT
    Area := _width * _height;
END_METHOD
END_CLASS

VAR_GLOBAL g_area : INT; END_VAR
PROGRAM Main
VAR b : Box; END_VAR
    b.SetSize(w := 7, h := 6);
    g_area := b.Area();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_area"), Some(&Value::Int(42)));
}

// =============================================================================
// 5. Pointer to FB instance variable, read through pointer
// =============================================================================

#[test]
fn pointer_to_local_read_after_fb_call() {
    // Take pointer to a local, call FB that modifies it, read through pointer
    let source = r#"
FUNCTION_BLOCK Incrementer
VAR_INPUT
    target : REF_TO INT;
END_VAR
    IF target <> NULL THEN
        target^ := target^ + 10;
    END_IF;
END_FUNCTION_BLOCK

VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    counter : INT := 0;
    inc : Incrementer;
END_VAR
    inc(target := REF(counter));
    g_val := counter;
END_PROGRAM
"#;
    let engine = run_program(source, 3);
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(30))); // 10*3
}

// =============================================================================
// 6. Multiple FB instances with independent state
// =============================================================================

#[test]
fn multiple_fb_instances_independent() {
    let source = r#"
FUNCTION_BLOCK Counter
VAR
    count : INT := 0;
END_VAR
VAR_OUTPUT
    value : INT;
END_VAR
    count := count + 1;
    value := count;
END_FUNCTION_BLOCK

VAR_GLOBAL g_a : INT; g_b : INT; END_VAR
PROGRAM Main
VAR
    a : Counter;
    b : Counter;
END_VAR
    a();
    a();
    a();
    b();
    g_a := a.value;
    g_b := b.value;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_a"), Some(&Value::Int(3)));
    assert_eq!(engine.vm().get_global("g_b"), Some(&Value::Int(1)));
}

// =============================================================================
// 7. Function with pointer param modifying caller's local
// =============================================================================

#[test]
fn function_modifies_caller_via_pointer() {
    let source = r#"
FUNCTION Triple : INT
VAR_INPUT
    p : REF_TO INT;
END_VAR
    p^ := p^ * 3;
    Triple := p^;
END_FUNCTION

VAR_GLOBAL g_x : INT; g_ret : INT; END_VAR
PROGRAM Main
VAR x : INT := 7; END_VAR
    g_ret := Triple(p := REF(x));
    g_x := x;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_x"), Some(&Value::Int(21)));
    assert_eq!(engine.vm().get_global("g_ret"), Some(&Value::Int(21)));
}

// =============================================================================
// 8. Pointer to global, modified by function
// =============================================================================

#[test]
fn function_modifies_global_via_pointer() {
    let source = r#"
FUNCTION AddTo : INT
VAR_INPUT
    target : REF_TO INT;
    amount : INT;
END_VAR
    target^ := target^ + amount;
    AddTo := target^;
END_FUNCTION

VAR_GLOBAL g_acc : INT; END_VAR
PROGRAM Main
VAR dummy : INT; END_VAR
    g_acc := 0;
    dummy := AddTo(target := REF(g_acc), amount := 100);
    dummy := AddTo(target := REF(g_acc), amount := 200);
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_acc"), Some(&Value::Int(300)));
}

// =============================================================================
// 9. Class method with pointer parameter
// =============================================================================

#[test]
fn class_method_pointer_param() {
    let source = r#"
CLASS Scaler
METHOD Scale
VAR_INPUT
    target : REF_TO INT;
    factor : INT;
END_VAR
    IF target <> NULL THEN
        target^ := target^ * factor;
    END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    s : Scaler;
    x : INT := 5;
END_VAR
    s.Scale(target := REF(x), factor := 8);
    g_val := x;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(40)));
}

// =============================================================================
// 10. FB field read + write round-trip across cycles
// =============================================================================

#[test]
fn fb_field_read_write_round_trip() {
    // Write to FB field, call it, read output field — across multiple cycles
    let source = r#"
FUNCTION_BLOCK Doubler
VAR_INPUT
    input : INT;
END_VAR
VAR_OUTPUT
    output : INT;
END_VAR
    output := input * 2;
END_FUNCTION_BLOCK

VAR_GLOBAL g_out : INT; END_VAR
PROGRAM Main
VAR
    d : Doubler;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    d.input := cycle * 10;
    d();
    g_out := d.output;
END_PROGRAM
"#;
    let engine = run_program(source, 3);
    // cycle 3: input = 30, output = 60
    assert_eq!(engine.vm().get_global("g_out"), Some(&Value::Int(60)));
}

// =============================================================================
// 11. Multiple field writes before call
// =============================================================================

#[test]
fn multiple_field_writes_before_call() {
    let source = r#"
FUNCTION_BLOCK Calc
VAR_INPUT
    a : INT;
    b : INT;
    op : INT;    // 0=add, 1=sub, 2=mul
END_VAR
VAR_OUTPUT
    result : INT;
END_VAR
    IF op = 0 THEN
        result := a + b;
    ELSIF op = 1 THEN
        result := a - b;
    ELSIF op = 2 THEN
        result := a * b;
    ELSE
        result := 0;
    END_IF;
END_FUNCTION_BLOCK

VAR_GLOBAL g_add : INT; g_mul : INT; END_VAR
PROGRAM Main
VAR calc : Calc; END_VAR
    calc.a := 7;
    calc.b := 6;
    calc.op := 0;
    calc();
    g_add := calc.result;

    calc.op := 2;
    calc();
    g_mul := calc.result;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_add"), Some(&Value::Int(13)));
    assert_eq!(engine.vm().get_global("g_mul"), Some(&Value::Int(42)));
}

// =============================================================================
// 12. Class with FB instance field
// =============================================================================

#[test]
fn class_using_fb_instance() {
    let source = r#"
FUNCTION_BLOCK Timer
VAR
    elapsed : INT := 0;
END_VAR
VAR_OUTPUT
    done : INT;
END_VAR
    elapsed := elapsed + 1;
    IF elapsed >= 5 THEN
        done := 1;
    ELSE
        done := 0;
    END_IF;
END_FUNCTION_BLOCK

CLASS Controller
VAR
    _enabled : BOOL := FALSE;
    _count : INT := 0;
END_VAR
METHOD Enable
    _enabled := TRUE;
END_METHOD
METHOD Tick
    IF _enabled THEN
        _count := _count + 1;
    END_IF;
END_METHOD
METHOD GetCount : INT
    GetCount := _count;
END_METHOD
END_CLASS

VAR_GLOBAL g_count : INT; END_VAR
PROGRAM Main
VAR
    ctrl : Controller;
END_VAR
    ctrl.Enable();
    ctrl.Tick();
    g_count := ctrl.GetCount();
END_PROGRAM
"#;
    let engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_count"), Some(&Value::Int(5)));
}

// =============================================================================
// 13. Complex: class that accumulates values via pointers across cycles
// =============================================================================

#[test]
fn class_accumulator_via_pointer_across_cycles() {
    let source = r#"
CLASS Sampler
VAR
    _sum : INT := 0;
    _count : INT := 0;
END_VAR
METHOD Sample
VAR_INPUT source : REF_TO INT; END_VAR
    IF source <> NULL THEN
        _sum := _sum + source^;
        _count := _count + 1;
    END_IF;
END_METHOD
METHOD GetAverage : INT
    IF _count > 0 THEN
        GetAverage := _sum / _count;
    ELSE
        GetAverage := 0;
    END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_avg : INT; END_VAR
PROGRAM Main
VAR
    sampler : Sampler;
    reading : INT := 0;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    reading := cycle * 10;    // 10, 20, 30, ...
    sampler.Sample(source := REF(reading));
    g_avg := sampler.GetAverage();
END_PROGRAM
"#;
    // After 4 cycles: readings = 10, 20, 30, 40
    // sum = 100, count = 4, avg = 25
    let engine = run_program(source, 4);
    assert_eq!(engine.vm().get_global("g_avg"), Some(&Value::Int(25)));
}
