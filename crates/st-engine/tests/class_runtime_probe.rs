//! Probing tests: verify actual runtime values for class method calls.

use st_ir::*;
use st_engine::*;

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

// =============================================================================
// Method return values
// =============================================================================

#[test]
fn method_return_value_is_correct() {
    let source = r#"
CLASS Adder
METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    calc : Adder;
END_VAR
    g_result := calc.Add(a := 10, b := 32);
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    let val = engine.vm().get_global("g_result");
    assert_eq!(val, Some(&Value::Int(42)), "10 + 32 = 42");
}

// =============================================================================
// State persists across method calls within one cycle
// =============================================================================

#[test]
fn method_mutates_class_state() {
    let source = r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
METHOD Increment
    count := count + 1;
END_METHOD
METHOD GetCount : INT
    GetCount := count;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_count : INT;
END_VAR

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c.Increment();
    c.Increment();
    c.Increment();
    g_count := c.GetCount();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    let val = engine.vm().get_global("g_count");
    assert_eq!(val, Some(&Value::Int(3)), "3 increments = 3");
}

// =============================================================================
// State persists across scan cycles
// =============================================================================

#[test]
fn class_state_persists_across_cycles() {
    let source = r#"
CLASS Accumulator
VAR
    total : INT := 0;
END_VAR
METHOD Add
VAR_INPUT
    value : INT;
END_VAR
    total := total + value;
END_METHOD
METHOD GetTotal : INT
    GetTotal := total;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_total : INT;
END_VAR

PROGRAM Main
VAR
    acc : Accumulator;
END_VAR
    acc.Add(value := 10);
    g_total := acc.GetTotal();
END_PROGRAM
"#;
    let engine = run_program(source, 5);
    let val = engine.vm().get_global("g_total");
    assert_eq!(val, Some(&Value::Int(50)), "10 added per cycle * 5 cycles = 50");
}

// =============================================================================
// Multiple class instances are independent
// =============================================================================

#[test]
fn multiple_instances_independent() {
    let source = r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
METHOD Increment
    count := count + 1;
END_METHOD
METHOD GetCount : INT
    GetCount := count;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_a : INT;
    g_b : INT;
END_VAR

PROGRAM Main
VAR
    a : Counter;
    b : Counter;
END_VAR
    a.Increment();
    a.Increment();
    a.Increment();
    b.Increment();
    g_a := a.GetCount();
    g_b := b.GetCount();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    let va = engine.vm().get_global("g_a");
    let vb = engine.vm().get_global("g_b");
    assert_eq!(va, Some(&Value::Int(3)), "a incremented 3 times");
    assert_eq!(vb, Some(&Value::Int(1)), "b incremented 1 time");
}

// =============================================================================
// Method with local variables and control flow
// =============================================================================

#[test]
fn method_with_control_flow_returns_correctly() {
    let source = r#"
CLASS MathHelper
METHOD Max : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    IF a > b THEN
        Max := a;
    ELSE
        Max := b;
    END_IF;
END_METHOD

METHOD Clamp : INT
VAR_INPUT
    val : INT;
    lo : INT;
    hi : INT;
END_VAR
    IF val < lo THEN
        Clamp := lo;
    ELSIF val > hi THEN
        Clamp := hi;
    ELSE
        Clamp := val;
    END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_max : INT;
    g_clamped_lo : INT;
    g_clamped_mid : INT;
    g_clamped_hi : INT;
END_VAR

PROGRAM Main
VAR
    m : MathHelper;
END_VAR
    g_max := m.Max(a := 7, b := 42);
    g_clamped_lo := m.Clamp(val := -5, lo := 0, hi := 100);
    g_clamped_mid := m.Clamp(val := 50, lo := 0, hi := 100);
    g_clamped_hi := m.Clamp(val := 200, lo := 0, hi := 100);
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_max"), Some(&Value::Int(42)));
    assert_eq!(engine.vm().get_global("g_clamped_lo"), Some(&Value::Int(0)));
    assert_eq!(engine.vm().get_global("g_clamped_mid"), Some(&Value::Int(50)));
    assert_eq!(engine.vm().get_global("g_clamped_hi"), Some(&Value::Int(100)));
}

// =============================================================================
// Method with loop
// =============================================================================

#[test]
fn method_with_for_loop() {
    let source = r#"
CLASS Factorial
METHOD Compute : INT
VAR_INPUT
    n : INT;
END_VAR
VAR
    result : INT;
    i : INT;
END_VAR
    result := 1;
    FOR i := 1 TO n DO
        result := result * i;
    END_FOR;
    Compute := result;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_fact5 : INT;
    g_fact7 : INT;
END_VAR

PROGRAM Main
VAR
    f : Factorial;
END_VAR
    g_fact5 := f.Compute(n := 5);
    g_fact7 := f.Compute(n := 7);
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_fact5"), Some(&Value::Int(120)));
    assert_eq!(engine.vm().get_global("g_fact7"), Some(&Value::Int(5040)));
}

// =============================================================================
// Inherited method dispatch
// =============================================================================

#[test]
fn inherited_method_returns_value() {
    let source = r#"
CLASS Base
METHOD GetBaseVal : INT
    GetBaseVal := 100;
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
METHOD GetDerivedVal : INT
    GetDerivedVal := 200;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_base : INT;
    g_derived : INT;
END_VAR

PROGRAM Main
VAR
    d : Derived;
END_VAR
    g_base := d.GetBaseVal();
    g_derived := d.GetDerivedVal();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_base"), Some(&Value::Int(100)));
    assert_eq!(engine.vm().get_global("g_derived"), Some(&Value::Int(200)));
}

// =============================================================================
// Inherited field access
// =============================================================================

#[test]
fn method_accesses_inherited_fields() {
    let source = r#"
CLASS Base
VAR
    baseVal : INT := 10;
END_VAR
METHOD GetBaseVal : INT
    GetBaseVal := baseVal;
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
VAR
    derivedVal : INT := 20;
END_VAR
METHOD GetSum : INT
    GetSum := baseVal + derivedVal;
END_METHOD
METHOD SetBaseVal
VAR_INPUT
    v : INT;
END_VAR
    baseVal := v;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_base : INT;
    g_sum  : INT;
    g_after : INT;
END_VAR

PROGRAM Main
VAR
    d : Derived;
END_VAR
    g_base := d.GetBaseVal();
    g_sum := d.GetSum();
    d.SetBaseVal(v := 100);
    g_after := d.GetSum();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_base"), Some(&Value::Int(10)), "inherited default");
    assert_eq!(engine.vm().get_global("g_sum"), Some(&Value::Int(30)), "10 + 20 = 30");
    assert_eq!(engine.vm().get_global("g_after"), Some(&Value::Int(120)), "100 + 20 = 120");
}

#[test]
fn three_level_inheritance_field_access() {
    let source = r#"
CLASS A
VAR
    aVal : INT := 1;
END_VAR
METHOD GetA : INT
    GetA := aVal;
END_METHOD
END_CLASS

CLASS B EXTENDS A
VAR
    bVal : INT := 10;
END_VAR
METHOD GetAB : INT
    GetAB := aVal + bVal;
END_METHOD
END_CLASS

CLASS C EXTENDS B
VAR
    cVal : INT := 100;
END_VAR
METHOD GetABC : INT
    GetABC := aVal + bVal + cVal;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_a : INT;
    g_ab : INT;
    g_abc : INT;
END_VAR

PROGRAM Main
VAR
    obj : C;
END_VAR
    g_a := obj.GetA();
    g_ab := obj.GetAB();
    g_abc := obj.GetABC();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_a"), Some(&Value::Int(1)));
    assert_eq!(engine.vm().get_global("g_ab"), Some(&Value::Int(11)));
    assert_eq!(engine.vm().get_global("g_abc"), Some(&Value::Int(111)));
}

// =============================================================================
// State persistence with inheritance
// =============================================================================

#[test]
fn inherited_state_persists_across_cycles() {
    let source = r#"
CLASS Base
VAR
    total : INT := 0;
END_VAR
METHOD AddBase
VAR_INPUT
    v : INT;
END_VAR
    total := total + v;
END_METHOD
METHOD GetTotal : INT
    GetTotal := total;
END_METHOD
END_CLASS

CLASS Child EXTENDS Base
VAR
    multiplier : INT := 2;
END_VAR
METHOD AddScaled
VAR_INPUT
    v : INT;
END_VAR
    total := total + v * multiplier;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_total : INT;
END_VAR

PROGRAM Main
VAR
    c : Child;
END_VAR
    c.AddScaled(v := 10);
    g_total := c.GetTotal();
END_PROGRAM
"#;
    let engine = run_program(source, 5);
    // Each cycle adds 10*2=20. After 5 cycles: 100
    assert_eq!(engine.vm().get_global("g_total"), Some(&Value::Int(100)));
}

// =============================================================================
// Void methods that mutate state
// =============================================================================

#[test]
fn void_method_mutates_state_and_getter_reads_it() {
    let source = r#"
CLASS Thermostat
VAR
    _setpoint : INT := 20;
    _current  : INT := 0;
    _heating  : BOOL := FALSE;
END_VAR
METHOD SetTarget
VAR_INPUT
    target : INT;
END_VAR
    _setpoint := target;
END_METHOD
METHOD Update
VAR_INPUT
    temperature : INT;
END_VAR
    _current := temperature;
    IF _current < _setpoint THEN
        _heating := TRUE;
    ELSE
        _heating := FALSE;
    END_IF;
END_METHOD
METHOD IsHeating : INT
    IF _heating THEN
        IsHeating := 1;
    ELSE
        IsHeating := 0;
    END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_heating_cold : INT;
    g_heating_hot  : INT;
END_VAR

PROGRAM Main
VAR
    t : Thermostat;
END_VAR
    t.SetTarget(target := 25);
    t.Update(temperature := 18);
    g_heating_cold := t.IsHeating();
    t.Update(temperature := 30);
    g_heating_hot := t.IsHeating();
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_heating_cold"), Some(&Value::Int(1)), "18 < 25 → heating");
    assert_eq!(engine.vm().get_global("g_heating_hot"), Some(&Value::Int(0)), "30 >= 25 → not heating");
}
