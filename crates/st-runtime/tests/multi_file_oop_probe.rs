//! Multi-file OOP probing tests.
//! Verify that classes, interfaces, inheritance, and methods work across file boundaries.

use st_ir::*;
use st_runtime::*;

/// Parse multiple source strings, compile, and run.
fn run_multi(sources: &[&str], cycles: u64) -> Engine {
    let parse_result = st_syntax::multi_file::parse_multi(sources);
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
// 1. Class defined in one file, used in another
// =============================================================================

#[test]
fn class_cross_file_instantiation() {
    let file_class = r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR c : Counter; END_VAR
    c.Inc();
    c.Inc();
    c.Inc();
    g_val := c.Get();
END_PROGRAM
"#;
    let e = run_multi(&[file_class, file_main], 1);
    assert_eq!(e.vm().get_global("g_val"), Some(&Value::Int(3)));
}

// =============================================================================
// 2. Interface in one file, implemented in another, used in a third
// =============================================================================

#[test]
fn interface_cross_file() {
    let file_interface = r#"
INTERFACE IResettable
    METHOD Reset
    END_METHOD
END_INTERFACE
"#;
    let file_class = r#"
CLASS Timer IMPLEMENTS IResettable
VAR _ticks : INT := 0; END_VAR
METHOD Tick _ticks := _ticks + 1; END_METHOD
METHOD Get : INT Get := _ticks; END_METHOD
METHOD Reset _ticks := 0; END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_ticks : INT; END_VAR
PROGRAM Main
VAR t : Timer; END_VAR
    t.Tick();
    g_ticks := t.Get();
END_PROGRAM
"#;
    let e = run_multi(&[file_interface, file_class, file_main], 5);
    assert_eq!(e.vm().get_global("g_ticks"), Some(&Value::Int(5)));
}

// =============================================================================
// 3. Inheritance across files — base in file A, derived in file B
// =============================================================================

#[test]
fn inheritance_cross_file() {
    let file_base = r#"
CLASS Animal
VAR _legs : INT := 0; END_VAR
METHOD SetLegs VAR_INPUT n : INT; END_VAR _legs := n; END_METHOD
METHOD GetLegs : INT GetLegs := _legs; END_METHOD
END_CLASS
"#;
    let file_derived = r#"
CLASS Dog EXTENDS Animal
VAR _name_code : INT := 0; END_VAR
METHOD SetName VAR_INPUT code : INT; END_VAR _name_code := code; END_METHOD
METHOD Describe : INT
    Describe := _legs * 100 + _name_code;
END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_desc : INT; END_VAR
PROGRAM Main
VAR d : Dog; END_VAR
    d.SetLegs(n := 4);
    d.SetName(code := 42);
    g_desc := d.Describe();
END_PROGRAM
"#;
    let e = run_multi(&[file_base, file_derived, file_main], 1);
    assert_eq!(e.vm().get_global("g_desc"), Some(&Value::Int(442))); // 4*100 + 42
}

// =============================================================================
// 4. Inherited method call across files
// =============================================================================

#[test]
fn inherited_method_cross_file() {
    let file_base = r#"
CLASS Base
METHOD Hello : INT
    Hello := 99;
END_METHOD
END_CLASS
"#;
    let file_derived = r#"
CLASS Child EXTENDS Base
METHOD World : INT
    World := 1;
END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_hello : INT; g_world : INT; END_VAR
PROGRAM Main
VAR c : Child; END_VAR
    g_hello := c.Hello();
    g_world := c.World();
END_PROGRAM
"#;
    let e = run_multi(&[file_base, file_derived, file_main], 1);
    assert_eq!(e.vm().get_global("g_hello"), Some(&Value::Int(99)));
    assert_eq!(e.vm().get_global("g_world"), Some(&Value::Int(1)));
}

// =============================================================================
// 5. Function in one file, class in another, program in a third
// =============================================================================

#[test]
fn function_class_program_three_files() {
    let file_util = r#"
FUNCTION DoubleIt : INT
VAR_INPUT v : INT; END_VAR
    DoubleIt := v * 2;
END_FUNCTION
"#;
    let file_class = r#"
CLASS Processor
VAR _result : INT := 0; END_VAR
METHOD Process
VAR_INPUT input : INT; END_VAR
    _result := DoubleIt(v := input);
END_METHOD
METHOD GetResult : INT
    GetResult := _result;
END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_result : INT; END_VAR
PROGRAM Main
VAR p : Processor; END_VAR
    p.Process(input := 21);
    g_result := p.GetResult();
END_PROGRAM
"#;
    let e = run_multi(&[file_util, file_class, file_main], 1);
    assert_eq!(e.vm().get_global("g_result"), Some(&Value::Int(42)));
}

// =============================================================================
// 6. Class with pointer method, defined across files
// =============================================================================

#[test]
fn class_pointer_method_cross_file() {
    let file_class = r#"
CLASS Writer
METHOD WriteTo
VAR_INPUT target : REF_TO INT; value : INT; END_VAR
    IF target <> NULL THEN target^ := value; END_IF;
END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    w : Writer;
    x : INT := 0;
END_VAR
    w.WriteTo(target := REF(x), value := 77);
    g_val := x;
END_PROGRAM
"#;
    let e = run_multi(&[file_class, file_main], 1);
    assert_eq!(e.vm().get_global("g_val"), Some(&Value::Int(77)));
}

// =============================================================================
// 7. FB in one file using class from another file
// =============================================================================

#[test]
fn fb_uses_class_cross_file() {
    let file_class = r#"
CLASS Accumulator
VAR _total : INT := 0; END_VAR
METHOD Add VAR_INPUT v : INT; END_VAR _total := _total + v; END_METHOD
METHOD Get : INT Get := _total; END_METHOD
END_CLASS
"#;
    let file_fb = r#"
FUNCTION_BLOCK Channel
VAR_INPUT raw : INT; END_VAR
VAR_OUTPUT avg : INT; END_VAR
VAR acc : Accumulator; count : INT := 0; END_VAR
    acc.Add(v := raw);
    count := count + 1;
    avg := acc.Get() / count;
END_FUNCTION_BLOCK
"#;
    let file_main = r#"
VAR_GLOBAL g_avg : INT; END_VAR
PROGRAM Main
VAR ch : Channel; END_VAR
    ch(raw := 100);
    g_avg := ch.avg;
END_PROGRAM
"#;
    let e = run_multi(&[file_class, file_fb, file_main], 5);
    assert_eq!(e.vm().get_global("g_avg"), Some(&Value::Int(100))); // constant input → avg=100
}

// =============================================================================
// 8. Two classes interacting across files
// =============================================================================

#[test]
fn two_classes_cross_file_interaction() {
    let file_producer = r#"
CLASS Producer
VAR _seq : INT := 0; END_VAR
METHOD Produce : INT _seq := _seq + 1; Produce := _seq; END_METHOD
END_CLASS
"#;
    let file_consumer = r#"
CLASS Consumer
VAR _total : INT := 0; END_VAR
METHOD Consume VAR_INPUT amount : INT; END_VAR _total := _total + amount; END_METHOD
METHOD GetTotal : INT GetTotal := _total; END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_total : INT; END_VAR
PROGRAM Main
VAR p : Producer; c : Consumer; END_VAR
    c.Consume(amount := p.Produce());
    g_total := c.GetTotal();
END_PROGRAM
"#;
    let e = run_multi(&[file_producer, file_consumer, file_main], 4);
    // Cycle 1: produce 1, consume 1, total=1
    // Cycle 2: produce 2, consume 2, total=3
    // Cycle 3: produce 3, consume 3, total=6
    // Cycle 4: produce 4, consume 4, total=10
    assert_eq!(e.vm().get_global("g_total"), Some(&Value::Int(10)));
}

// =============================================================================
// 9. Abstract class in one file, concrete in another
// =============================================================================

#[test]
fn abstract_class_cross_file() {
    let file_abstract = r#"
ABSTRACT CLASS Shape
ABSTRACT METHOD Area : INT
END_METHOD
METHOD Describe : INT
    Describe := 1;
END_METHOD
END_CLASS
"#;
    let file_concrete = r#"
CLASS Square EXTENDS Shape
VAR _side : INT := 0; END_VAR
METHOD SetSide VAR_INPUT s : INT; END_VAR _side := s; END_METHOD
OVERRIDE METHOD Area : INT
    Area := _side * _side;
END_METHOD
END_CLASS
"#;
    let file_main = r#"
VAR_GLOBAL g_area : INT; g_desc : INT; END_VAR
PROGRAM Main
VAR sq : Square; END_VAR
    sq.SetSide(s := 7);
    g_area := sq.Area();
    g_desc := sq.Describe();
END_PROGRAM
"#;
    let e = run_multi(&[file_abstract, file_concrete, file_main], 1);
    assert_eq!(e.vm().get_global("g_area"), Some(&Value::Int(49)));
    assert_eq!(e.vm().get_global("g_desc"), Some(&Value::Int(1)));
}

// =============================================================================
// 10. File order independence (reverse order should also work)
// =============================================================================

#[test]
fn file_order_independence() {
    // Main references class that hasn't been parsed yet (forward reference)
    let file_main = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR c : Counter; END_VAR
    c.Inc();
    g_val := c.Get();
END_PROGRAM
"#;
    let file_class = r#"
CLASS Counter
VAR _count : INT := 0; END_VAR
METHOD Inc _count := _count + 1; END_METHOD
METHOD Get : INT Get := _count; END_METHOD
END_CLASS
"#;
    // Class defined AFTER main — forward reference test
    let e = run_multi(&[file_main, file_class], 3);
    assert_eq!(e.vm().get_global("g_val"), Some(&Value::Int(3)));
}
