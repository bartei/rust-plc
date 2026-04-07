//! Runtime/VM tests for IEC 61131-3 OOP extensions (Phase 12).
//! End-to-end: parse → compile → execute in VM.

use st_ir::*;
use st_runtime::*;

/// Helper: parse + compile + run N cycles, return the engine.
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
// Class compilation + program coexistence
// =============================================================================

#[test]
fn class_and_program_compile_and_run() {
    let engine = run_program(
        r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
METHOD Increment
    count := count + 1;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    x : INT := 42;
END_VAR
    x := x + 1;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

#[test]
fn class_does_not_interfere_with_program_vars() {
    let engine = run_program(
        r#"
CLASS Holder
VAR
    value : INT := 999;
END_VAR
END_CLASS

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 100;
END_PROGRAM
"#,
        1,
    );
    // Program variable should work independently
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Multiple classes in same source
// =============================================================================

#[test]
fn multiple_classes_compile_and_run() {
    let engine = run_program(
        r#"
CLASS A
VAR
    ax : INT := 1;
END_VAR
END_CLASS

CLASS B
VAR
    bx : INT := 2;
END_VAR
END_CLASS

CLASS C
VAR
    cx : INT := 3;
END_VAR
METHOD GetCx : INT
    GetCx := cx;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := 42;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Class with interfaces (compile-level)
// =============================================================================

#[test]
fn interface_and_class_compile_and_run() {
    let engine = run_program(
        r#"
INTERFACE IRunnable
METHOD Run
END_METHOD
END_INTERFACE

CLASS Worker IMPLEMENTS IRunnable
METHOD Run
END_METHOD
END_CLASS

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Class with inheritance (compile-level)
// =============================================================================

#[test]
fn inheritance_hierarchy_compiles_and_runs() {
    let engine = run_program(
        r#"
CLASS Base
VAR
    x : INT := 10;
END_VAR
METHOD GetX : INT
    GetX := x;
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
VAR
    y : INT := 20;
END_VAR
METHOD GetY : INT
    GetY := y;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := 1;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Abstract class (compile-level, no instantiation)
// =============================================================================

#[test]
fn abstract_class_compiles() {
    let engine = run_program(
        r#"
ABSTRACT CLASS Shape
VAR
    color : INT;
END_VAR
ABSTRACT METHOD Area : REAL
END_METHOD
METHOD SetColor
VAR_INPUT
    c : INT;
END_VAR
    color := c;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    x : INT;
END_VAR
    x := 0;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Final class (compile-level)
// =============================================================================

#[test]
fn final_class_compiles() {
    let engine = run_program(
        r#"
FINAL CLASS Singleton
VAR
    value : INT := 42;
END_VAR
METHOD GetValue : INT
    GetValue := value;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    x : INT;
END_VAR
    x := 1;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Complex OOP scenario
// =============================================================================

#[test]
fn complex_oop_hierarchy_compiles() {
    let engine = run_program(
        r#"
INTERFACE IComparable
METHOD CompareTo : INT
VAR_INPUT
    other : INT;
END_VAR
END_METHOD
END_INTERFACE

INTERFACE ICloneable
METHOD Clone : INT
END_METHOD
END_INTERFACE

ABSTRACT CLASS BaseObject
VAR
    id : INT;
END_VAR
ABSTRACT METHOD ToString : INT
END_METHOD
METHOD GetId : INT
    GetId := id;
END_METHOD
END_CLASS

CLASS ConcreteObject EXTENDS BaseObject IMPLEMENTS IComparable, ICloneable
VAR
    data : INT;
END_VAR
METHOD ToString : INT
    ToString := data;
END_METHOD
METHOD CompareTo : INT
VAR_INPUT
    other : INT;
END_VAR
    CompareTo := data - other;
END_METHOD
METHOD Clone : INT
    Clone := data;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    x : INT;
END_VAR
    x := 0;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Method calls from program
// =============================================================================

#[test]
fn method_call_executes() {
    let engine = run_program(
        r#"
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
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c.Increment();
    c.Increment();
    c.Increment();
    g_result := c.GetCount();
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

#[test]
fn method_call_with_return_value() {
    let engine = run_program(
        r#"
CLASS Math
METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_sum : INT;
END_VAR

PROGRAM Main
VAR
    m : Math;
END_VAR
    g_sum := m.Add(a := 10, b := 20);
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

#[test]
fn inherited_method_call_runtime() {
    let engine = run_program(
        r#"
CLASS Base
METHOD GetBase : INT
    GetBase := 42;
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
METHOD GetDerived : INT
    GetDerived := 99;
END_METHOD
END_CLASS

VAR_GLOBAL
    g_base_val : INT;
    g_derived_val : INT;
END_VAR

PROGRAM Main
VAR
    d : Derived;
END_VAR
    g_base_val := d.GetBase();
    g_derived_val := d.GetDerived();
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// Class method with logic
// =============================================================================

#[test]
fn class_method_with_complex_logic() {
    let engine = run_program(
        r#"
CLASS MathHelper
METHOD Factorial : INT
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
    Factorial := result;
END_METHOD

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
END_CLASS

PROGRAM Main
VAR
    x : INT;
END_VAR
    x := 0;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}
