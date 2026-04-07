//! Probing tests: class instantiation patterns.
//! Tests multiple instances, composition, lifecycle, and cross-scope behavior.

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

// =============================================================================
// 1. Multiple instances, independent state, verified values
// =============================================================================

#[test]
fn three_independent_counters() {
    let source = r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS

VAR_GLOBAL g_a : INT; g_b : INT; g_c : INT; END_VAR
PROGRAM Main
VAR a : Counter; b : Counter; c : Counter; END_VAR
    a.Inc(); a.Inc(); a.Inc(); a.Inc(); a.Inc();   // 5
    b.Inc(); b.Inc(); b.Inc();                     // 3
    c.Inc();                                       // 1
    g_a := a.Get();
    g_b := b.Get();
    g_c := c.Get();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_a"), Some(&Value::Int(5)));
    assert_eq!(e.vm().get_global("g_b"), Some(&Value::Int(3)));
    assert_eq!(e.vm().get_global("g_c"), Some(&Value::Int(1)));
}

// =============================================================================
// 2. Multiple instances accumulate independently across cycles
// =============================================================================

#[test]
fn multiple_instances_across_cycles() {
    let source = r#"
CLASS Accumulator
VAR _total : INT := 0; END_VAR
METHOD Add VAR_INPUT v : INT; END_VAR _total := _total + v; END_METHOD
METHOD Get : INT Get := _total; END_METHOD
END_CLASS

VAR_GLOBAL g_fast : INT; g_slow : INT; END_VAR
PROGRAM Main
VAR fast : Accumulator; slow : Accumulator; cycle : INT := 0; END_VAR
    cycle := cycle + 1;
    fast.Add(v := 10);                         // 10 per cycle
    IF (cycle MOD 3) = 0 THEN
        slow.Add(v := 100);                    // 100 every 3rd cycle
    END_IF;
    g_fast := fast.Get();
    g_slow := slow.Get();
END_PROGRAM
"#;
    let e = run_program(source, 9);
    assert_eq!(e.vm().get_global("g_fast"), Some(&Value::Int(90)));   // 10*9
    assert_eq!(e.vm().get_global("g_slow"), Some(&Value::Int(300)));  // 100*3
}

// =============================================================================
// 3. Class instance inside a FUNCTION_BLOCK
// =============================================================================

#[test]
fn class_instance_inside_fb() {
    let source = r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS

FUNCTION_BLOCK Wrapper
VAR
    inner : Counter;
END_VAR
VAR_OUTPUT
    value : INT;
END_VAR
    inner.Inc();
    value := inner.Get();
END_FUNCTION_BLOCK

VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR w : Wrapper; END_VAR
    w();
    g_val := w.value;
END_PROGRAM
"#;
    let e = run_program(source, 5);
    assert_eq!(e.vm().get_global("g_val"), Some(&Value::Int(5)));
}

// =============================================================================
// 4. Class instance inside another class (composition)
// =============================================================================

#[test]
fn class_composition() {
    let source = r#"
CLASS Engine
VAR _rpm : INT := 0; END_VAR
METHOD SetRpm VAR_INPUT rpm : INT; END_VAR _rpm := rpm; END_METHOD
METHOD GetRpm : INT GetRpm := _rpm; END_METHOD
END_CLASS

CLASS Car
VAR
    _speed : INT := 0;
END_VAR
METHOD Accelerate
    _speed := _speed + 10;
END_METHOD
METHOD GetSpeed : INT
    GetSpeed := _speed;
END_METHOD
END_CLASS

VAR_GLOBAL g_speed : INT; END_VAR
PROGRAM Main
VAR car : Car; END_VAR
    car.Accelerate();
    car.Accelerate();
    car.Accelerate();
    g_speed := car.GetSpeed();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_speed"), Some(&Value::Int(30)));
}

// =============================================================================
// 5. Explicit init method pattern (constructor simulation)
// =============================================================================

#[test]
fn explicit_init_method() {
    let source = r#"
CLASS Rect
VAR
    _w : INT := 0;
    _h : INT := 0;
END_VAR
METHOD Init
VAR_INPUT w : INT; h : INT; END_VAR
    _w := w;
    _h := h;
END_METHOD
METHOD Area : INT
    Area := _w * _h;
END_METHOD
END_CLASS

VAR_GLOBAL g_a1 : INT; g_a2 : INT; END_VAR
PROGRAM Main
VAR r1 : Rect; r2 : Rect; END_VAR
    r1.Init(w := 3, h := 4);
    r2.Init(w := 10, h := 5);
    g_a1 := r1.Area();
    g_a2 := r2.Area();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_a1"), Some(&Value::Int(12)));
    assert_eq!(e.vm().get_global("g_a2"), Some(&Value::Int(50)));
}

// =============================================================================
// 6. Init on first cycle, use on subsequent cycles
// =============================================================================

#[test]
fn init_once_use_many() {
    let source = r#"
CLASS StatefulWorker
VAR
    _configured : BOOL := FALSE;
    _multiplier : INT := 1;
    _total : INT := 0;
END_VAR
METHOD Configure
VAR_INPUT mult : INT; END_VAR
    _multiplier := mult;
    _configured := TRUE;
END_METHOD
METHOD Process
VAR_INPUT value : INT; END_VAR
    IF _configured THEN
        _total := _total + value * _multiplier;
    END_IF;
END_METHOD
METHOD GetTotal : INT
    GetTotal := _total;
END_METHOD
END_CLASS

VAR_GLOBAL g_total : INT; END_VAR
PROGRAM Main
VAR
    worker : StatefulWorker;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    IF cycle = 1 THEN
        worker.Configure(mult := 3);
    END_IF;
    worker.Process(value := 10);
    g_total := worker.GetTotal();
END_PROGRAM
"#;
    // Each cycle adds 10*3=30 (after configure on cycle 1)
    let e = run_program(source, 5);
    assert_eq!(e.vm().get_global("g_total"), Some(&Value::Int(150)));
}

// =============================================================================
// 7. Class with default initial values
// =============================================================================

#[test]
fn class_default_values() {
    let source = r#"
CLASS Defaults
VAR
    _a : INT := 42;
    _b : INT := 100;
    _c : BOOL := TRUE;
END_VAR
METHOD GetA : INT GetA := _a; END_METHOD
METHOD GetB : INT GetB := _b; END_METHOD
METHOD GetC : INT
    IF _c THEN GetC := 1; ELSE GetC := 0; END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_a : INT; g_b : INT; g_c : INT; END_VAR
PROGRAM Main
VAR d : Defaults; END_VAR
    g_a := d.GetA();
    g_b := d.GetB();
    g_c := d.GetC();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_a"), Some(&Value::Int(42)));
    assert_eq!(e.vm().get_global("g_b"), Some(&Value::Int(100)));
    assert_eq!(e.vm().get_global("g_c"), Some(&Value::Int(1)));
}

// =============================================================================
// 8. Inherited class with default values at each level
// =============================================================================

#[test]
fn inherited_defaults() {
    let source = r#"
CLASS Base
VAR _base : INT := 10; END_VAR
METHOD GetBase : INT GetBase := _base; END_METHOD
END_CLASS

CLASS Child EXTENDS Base
VAR _child : INT := 20; END_VAR
METHOD GetChild : INT GetChild := _child; END_METHOD
METHOD GetSum : INT GetSum := _base + _child; END_METHOD
END_CLASS

VAR_GLOBAL g_base : INT; g_child : INT; g_sum : INT; END_VAR
PROGRAM Main
VAR c : Child; END_VAR
    g_base := c.GetBase();
    g_child := c.GetChild();
    g_sum := c.GetSum();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_base"), Some(&Value::Int(10)));
    assert_eq!(e.vm().get_global("g_child"), Some(&Value::Int(20)));
    assert_eq!(e.vm().get_global("g_sum"), Some(&Value::Int(30)));
}

// =============================================================================
// 9. Reset pattern — reinitialize instance mid-run
// =============================================================================

#[test]
fn reset_and_reuse() {
    let source = r#"
CLASS Bucket
VAR _level : INT := 0; END_VAR
METHOD Fill VAR_INPUT amount : INT; END_VAR _level := _level + amount; END_METHOD
METHOD Drain _level := 0; END_METHOD
METHOD GetLevel : INT GetLevel := _level; END_METHOD
END_CLASS

VAR_GLOBAL g_level : INT; END_VAR
PROGRAM Main
VAR
    bucket : Bucket;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    bucket.Fill(amount := 5);
    IF (cycle MOD 4) = 0 THEN
        bucket.Drain();
    END_IF;
    g_level := bucket.GetLevel();
END_PROGRAM
"#;
    // Cycles 1-3: 5, 10, 15
    // Cycle 4: 20 then drain → 0
    // Cycles 5-7: 5, 10, 15
    // Cycle 8: 20 then drain → 0
    let e = run_program(source, 3);
    assert_eq!(e.vm().get_global("g_level"), Some(&Value::Int(15)));

    let e = run_program(source, 4);
    assert_eq!(e.vm().get_global("g_level"), Some(&Value::Int(0)));

    let e = run_program(source, 7);
    assert_eq!(e.vm().get_global("g_level"), Some(&Value::Int(15)));
}

// =============================================================================
// 10. Multiple classes interacting
// =============================================================================

#[test]
fn two_classes_interacting() {
    let source = r#"
CLASS Producer
VAR _value : INT := 0; END_VAR
METHOD Produce : INT
    _value := _value + 7;
    Produce := _value;
END_METHOD
END_CLASS

CLASS Consumer
VAR _consumed : INT := 0; END_VAR
METHOD Consume VAR_INPUT amount : INT; END_VAR
    _consumed := _consumed + amount;
END_METHOD
METHOD GetConsumed : INT GetConsumed := _consumed; END_METHOD
END_CLASS

VAR_GLOBAL g_produced : INT; g_consumed : INT; END_VAR
PROGRAM Main
VAR
    p : Producer;
    c : Consumer;
    item : INT;
END_VAR
    item := p.Produce();
    c.Consume(amount := item);
    g_produced := item;
    g_consumed := c.GetConsumed();
END_PROGRAM
"#;
    // Each cycle: producer makes 7 more, consumer eats it all
    // Cycle 1: produce 7, consume 7
    // Cycle 2: produce 14, consume 7+14=21
    // Cycle 3: produce 21, consume 21+21=42
    let e = run_program(source, 3);
    assert_eq!(e.vm().get_global("g_produced"), Some(&Value::Int(21)));
    assert_eq!(e.vm().get_global("g_consumed"), Some(&Value::Int(42)));
}
