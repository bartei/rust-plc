//! Advanced class instantiation probes: harder scenarios.

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
// 1. Class instance receiving pointer, modifying external state
// =============================================================================

#[test]
fn class_modifies_external_via_pointer() {
    let source = r#"
CLASS Writer
VAR _written : INT := 0; END_VAR
METHOD WriteToTarget
VAR_INPUT target : REF_TO INT; value : INT; END_VAR
    IF target <> NULL THEN
        target^ := value;
        _written := _written + 1;
    END_IF;
END_METHOD
METHOD GetWriteCount : INT GetWriteCount := _written; END_METHOD
END_CLASS

VAR_GLOBAL g_x : INT; g_y : INT; g_writes : INT; END_VAR
PROGRAM Main
VAR
    w : Writer;
    x : INT := 0;
    y : INT := 0;
END_VAR
    w.WriteToTarget(target := REF(x), value := 42);
    w.WriteToTarget(target := REF(y), value := 99);
    g_x := x;
    g_y := y;
    g_writes := w.GetWriteCount();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_x"), Some(&Value::Int(42)));
    assert_eq!(e.vm().get_global("g_y"), Some(&Value::Int(99)));
    assert_eq!(e.vm().get_global("g_writes"), Some(&Value::Int(2)));
}

// =============================================================================
// 2. Two instances of derived class, each independent
// =============================================================================

#[test]
fn two_derived_instances_independent() {
    let source = r#"
CLASS Base
VAR _base : INT := 0; END_VAR
METHOD SetBase VAR_INPUT v : INT; END_VAR _base := v; END_METHOD
METHOD GetBase : INT GetBase := _base; END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
VAR _extra : INT := 0; END_VAR
METHOD SetExtra VAR_INPUT v : INT; END_VAR _extra := v; END_METHOD
METHOD GetSum : INT GetSum := _base + _extra; END_METHOD
END_CLASS

VAR_GLOBAL g_s1 : INT; g_s2 : INT; END_VAR
PROGRAM Main
VAR d1 : Derived; d2 : Derived; END_VAR
    d1.SetBase(v := 10);
    d1.SetExtra(v := 1);
    d2.SetBase(v := 100);
    d2.SetExtra(v := 2);
    g_s1 := d1.GetSum();
    g_s2 := d2.GetSum();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_s1"), Some(&Value::Int(11)));
    assert_eq!(e.vm().get_global("g_s2"), Some(&Value::Int(102)));
}

// =============================================================================
// 3. Class instances in FB, each FB instance has its own class state
// =============================================================================

#[test]
fn fb_with_class_instance_two_fbs() {
    let source = r#"
CLASS Ticker
VAR _ticks : INT := 0; END_VAR
METHOD Tick _ticks := _ticks + 1; END_METHOD
METHOD Get : INT Get := _ticks; END_METHOD
END_CLASS

FUNCTION_BLOCK Monitor
VAR
    ticker : Ticker;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
    ticker.Tick();
    count := ticker.Get();
END_FUNCTION_BLOCK

VAR_GLOBAL g_m1 : INT; g_m2 : INT; END_VAR
PROGRAM Main
VAR m1 : Monitor; m2 : Monitor; END_VAR
    m1();
    m1();
    m1();
    m2();
    g_m1 := m1.count;
    g_m2 := m2.count;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_m1"), Some(&Value::Int(3)));
    assert_eq!(e.vm().get_global("g_m2"), Some(&Value::Int(1)));
}

// =============================================================================
// 4. Class state machine — lifecycle pattern
// =============================================================================

#[test]
fn class_state_machine_lifecycle() {
    let source = r#"
CLASS StateMachine
VAR
    _state : INT := 0;     // 0=idle, 1=running, 2=done
    _counter : INT := 0;
    _target : INT := 0;
END_VAR
METHOD Start
VAR_INPUT target : INT; END_VAR
    _state := 1;
    _counter := 0;
    _target := target;
END_METHOD
METHOD Update : INT
    IF _state = 1 THEN
        _counter := _counter + 1;
        IF _counter >= _target THEN
            _state := 2;
        END_IF;
    END_IF;
    Update := _state;
END_METHOD
METHOD IsDone : INT
    IF _state = 2 THEN IsDone := 1; ELSE IsDone := 0; END_IF;
END_METHOD
METHOD Reset
    _state := 0;
    _counter := 0;
END_METHOD
END_CLASS

VAR_GLOBAL g_state : INT; g_done : INT; END_VAR
PROGRAM Main
VAR
    sm : StateMachine;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    IF cycle = 1 THEN
        sm.Start(target := 3);
    END_IF;
    g_state := sm.Update();
    g_done := sm.IsDone();
END_PROGRAM
"#;
    // Cycle 1: Start(3), Update: counter=1, state=1
    // Cycle 2: Update: counter=2, state=1
    // Cycle 3: Update: counter=3, state=2 (done!)
    let e = run_program(source, 2);
    assert_eq!(e.vm().get_global("g_state"), Some(&Value::Int(1)));
    assert_eq!(e.vm().get_global("g_done"), Some(&Value::Int(0)));

    let e = run_program(source, 3);
    assert_eq!(e.vm().get_global("g_state"), Some(&Value::Int(2)));
    assert_eq!(e.vm().get_global("g_done"), Some(&Value::Int(1)));
}

// =============================================================================
// 5. Builder pattern — chain multiple configure calls
// =============================================================================

#[test]
fn builder_pattern() {
    let source = r#"
CLASS PidConfig
VAR
    _kp : INT := 10;
    _ki : INT := 1;
    _kd : INT := 0;
    _outMin : INT := 0;
    _outMax : INT := 100;
END_VAR
METHOD SetKp VAR_INPUT v : INT; END_VAR _kp := v; END_METHOD
METHOD SetKi VAR_INPUT v : INT; END_VAR _ki := v; END_METHOD
METHOD SetKd VAR_INPUT v : INT; END_VAR _kd := v; END_METHOD
METHOD SetLimits VAR_INPUT lo : INT; hi : INT; END_VAR
    _outMin := lo; _outMax := hi;
END_METHOD
METHOD Compute : INT
VAR_INPUT error : INT; END_VAR
VAR raw : INT; END_VAR
    raw := _kp * error / 10;
    IF raw < _outMin THEN raw := _outMin; END_IF;
    IF raw > _outMax THEN raw := _outMax; END_IF;
    Compute := raw;
END_METHOD
END_CLASS

VAR_GLOBAL g_out : INT; END_VAR
PROGRAM Main
VAR pid : PidConfig; END_VAR
    pid.SetKp(v := 20);
    pid.SetLimits(lo := 0, hi := 50);
    g_out := pid.Compute(error := 30);
    // raw = 20*30/10 = 60, clamped to 50
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_out"), Some(&Value::Int(50)));
}

// =============================================================================
// 6. Observer pattern — class writes to multiple targets via pointers
// =============================================================================

#[test]
fn observer_pattern_via_pointers() {
    let source = r#"
CLASS Broadcaster
VAR
    _value : INT := 0;
END_VAR
METHOD SetValue VAR_INPUT v : INT; END_VAR
    _value := v;
END_METHOD
METHOD Notify
VAR_INPUT
    p1 : REF_TO INT;
    p2 : REF_TO INT;
    p3 : REF_TO INT;
END_VAR
    IF p1 <> NULL THEN p1^ := _value; END_IF;
    IF p2 <> NULL THEN p2^ := _value; END_IF;
    IF p3 <> NULL THEN p3^ := _value; END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_a : INT; g_b : INT; g_c : INT; END_VAR
PROGRAM Main
VAR
    bc : Broadcaster;
    a : INT := 0;
    b : INT := 0;
    c : INT := 0;
END_VAR
    bc.SetValue(v := 77);
    bc.Notify(p1 := REF(a), p2 := REF(b), p3 := REF(c));
    g_a := a;
    g_b := b;
    g_c := c;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_a"), Some(&Value::Int(77)));
    assert_eq!(e.vm().get_global("g_b"), Some(&Value::Int(77)));
    assert_eq!(e.vm().get_global("g_c"), Some(&Value::Int(77)));
}
