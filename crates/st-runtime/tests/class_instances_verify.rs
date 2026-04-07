//! Verification tests for playground/14_class_instances.st patterns.

use st_ir::*;
use st_runtime::*;

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

#[test]
fn verify_independent_accumulators() {
    let source = r#"
CLASS Accumulator
VAR _total : INT := 0; END_VAR
METHOD Add VAR_INPUT v : INT; END_VAR _total := _total + v; END_METHOD
METHOD GetTotal : INT GetTotal := _total; END_METHOD
METHOD Reset _total := 0; END_METHOD
END_CLASS

VAR_GLOBAL g_fast : INT; g_slow : INT; END_VAR
PROGRAM Main
VAR fast : Accumulator; slow : Accumulator; cycle : INT := 0; END_VAR
    cycle := cycle + 1;
    fast.Add(v := 10);
    IF (cycle MOD 3) = 0 THEN slow.Add(v := 100); END_IF;
    IF (cycle MOD 10) = 0 THEN fast.Reset(); END_IF;
    g_fast := fast.GetTotal();
    g_slow := slow.GetTotal();
END_PROGRAM
"#;
    // 9 cycles: fast = 10*9=90 (no reset yet), slow = 100*3=300
    let e = run_program(source, 9);
    assert_eq!(e.vm().get_global("g_fast"), Some(&Value::Int(90)));
    assert_eq!(e.vm().get_global("g_slow"), Some(&Value::Int(300)));

    // 10 cycles: fast resets at cycle 10 then adds 10 → 10, slow = 100*3=300 still
    // Wait — reset happens, then Add happens in same cycle?
    // Looking at code: Add first, then reset. So at cycle 10: added 10 (total=100), then reset (total=0)
    // But g_fast reads AFTER reset. So g_fast = 0
    let e = run_program(source, 10);
    assert_eq!(e.vm().get_global("g_fast"), Some(&Value::Int(0)));
}

#[test]
fn verify_producer_consumer() {
    let source = r#"
CLASS Producer
VAR _seq : INT := 0; END_VAR
METHOD Produce : INT _seq := _seq + 7; Produce := _seq; END_METHOD
END_CLASS
CLASS Consumer
VAR _total : INT := 0; END_VAR
METHOD Consume VAR_INPUT amount : INT; END_VAR _total := _total + amount; END_METHOD
METHOD GetTotal : INT GetTotal := _total; END_METHOD
END_CLASS

VAR_GLOBAL g_produced : INT; g_consumed : INT; END_VAR
PROGRAM Main
VAR p : Producer; c : Consumer; item : INT; END_VAR
    item := p.Produce();
    c.Consume(amount := item);
    g_produced := item;
    g_consumed := c.GetTotal();
END_PROGRAM
"#;
    // Cycle 1: produce 7, consume 7
    // Cycle 2: produce 14, consume 7+14=21
    // Cycle 3: produce 21, consume 21+21=42
    let e = run_program(source, 3);
    assert_eq!(e.vm().get_global("g_produced"), Some(&Value::Int(21)));
    assert_eq!(e.vm().get_global("g_consumed"), Some(&Value::Int(42)));
}

#[test]
fn verify_observer_broadcast() {
    let source = r#"
CLASS Broadcaster
VAR _value : INT := 0; END_VAR
METHOD SetValue VAR_INPUT v : INT; END_VAR _value := v; END_METHOD
METHOD Notify
VAR_INPUT p1 : REF_TO INT; p2 : REF_TO INT; p3 : REF_TO INT; END_VAR
    IF p1 <> NULL THEN p1^ := _value; END_IF;
    IF p2 <> NULL THEN p2^ := _value; END_IF;
    IF p3 <> NULL THEN p3^ := _value; END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_a : INT; g_b : INT; g_c : INT; END_VAR
PROGRAM Main
VAR bc : Broadcaster; a : INT; b : INT; c : INT; cycle : INT := 0; END_VAR
    cycle := cycle + 1;
    bc.SetValue(v := cycle * 11);
    bc.Notify(p1 := REF(a), p2 := REF(b), p3 := REF(c));
    g_a := a; g_b := b; g_c := c;
END_PROGRAM
"#;
    let e = run_program(source, 5);
    assert_eq!(e.vm().get_global("g_a"), Some(&Value::Int(55)));  // 5*11
    assert_eq!(e.vm().get_global("g_b"), Some(&Value::Int(55)));
    assert_eq!(e.vm().get_global("g_c"), Some(&Value::Int(55)));
}

#[test]
fn verify_fb_with_class_inside() {
    let source = r#"
CLASS Accumulator
VAR _total : INT := 0; _count : INT := 0; END_VAR
METHOD Add VAR_INPUT v : INT; END_VAR _total := _total + v; _count := _count + 1; END_METHOD
METHOD GetAverage : INT
    IF _count > 0 THEN GetAverage := _total / _count; ELSE GetAverage := 0; END_IF;
END_METHOD
METHOD GetCount : INT GetCount := _count; END_METHOD
END_CLASS

FUNCTION_BLOCK SensorChannel
VAR_INPUT rawValue : INT; END_VAR
VAR_OUTPUT filtered : INT; sampleCount : INT; END_VAR
VAR stats : Accumulator; END_VAR
    stats.Add(v := rawValue);
    filtered := stats.GetAverage();
    sampleCount := stats.GetCount();
END_FUNCTION_BLOCK

VAR_GLOBAL g_ch1 : INT; g_ch2 : INT; g_s1 : INT; g_s2 : INT; END_VAR
PROGRAM Main
VAR ch1 : SensorChannel; ch2 : SensorChannel; END_VAR
    ch1(rawValue := 100);
    ch2(rawValue := 200);
    g_ch1 := ch1.filtered;
    g_ch2 := ch2.filtered;
    g_s1 := ch1.sampleCount;
    g_s2 := ch2.sampleCount;
END_PROGRAM
"#;
    // After 5 cycles: ch1 always gets 100, avg=100; ch2 always gets 200, avg=200
    let e = run_program(source, 5);
    assert_eq!(e.vm().get_global("g_ch1"), Some(&Value::Int(100)));
    assert_eq!(e.vm().get_global("g_ch2"), Some(&Value::Int(200)));
    assert_eq!(e.vm().get_global("g_s1"), Some(&Value::Int(5)));
    assert_eq!(e.vm().get_global("g_s2"), Some(&Value::Int(5)));
}

#[test]
fn verify_inherited_controller() {
    let source = r#"
CLASS BaseController
VAR _output : INT := 0; _enabled : BOOL := FALSE; END_VAR
METHOD Enable _enabled := TRUE; END_METHOD
METHOD GetOutput : INT GetOutput := _output; END_METHOD
END_CLASS

CLASS BangBang EXTENDS BaseController
VAR _sp : INT := 50; _hyst : INT := 5; END_VAR
METHOD Configure VAR_INPUT sp : INT; hyst : INT; END_VAR _sp := sp; _hyst := hyst; END_METHOD
METHOD Compute VAR_INPUT pv : INT; END_VAR
    IF _enabled THEN
        IF pv < (_sp - _hyst) THEN _output := 100;
        ELSIF pv > (_sp + _hyst) THEN _output := 0; END_IF;
    ELSE _output := 0; END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_out : INT; END_VAR
PROGRAM Main
VAR ctrl : BangBang; END_VAR
    ctrl.Configure(sp := 50, hyst := 5);
    ctrl.Enable();
    ctrl.Compute(pv := 30);    // 30 < 45 → output=100
    g_out := ctrl.GetOutput();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_out"), Some(&Value::Int(100)));
}
