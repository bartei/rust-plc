//! Compiler tests for IEC 61131-3 OOP extensions (Phase 12).

use st_ir::{Module, PouKind};

fn compile_ok(source: &str) -> Module {
    let result = st_syntax::parse(source);
    assert!(
        result.errors.is_empty(),
        "Unexpected parse errors: {:?}",
        result.errors
    );
    st_compiler::compile(&result.source_file).expect("Compilation failed")
}

fn find_func<'a>(module: &'a Module, name: &str) -> &'a st_ir::Function {
    module
        .functions
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| panic!("Function '{name}' not found in module"))
}

// =============================================================================
// Basic class compilation
// =============================================================================

#[test]
fn compile_empty_class() {
    let m = compile_ok(r#"
CLASS Empty
END_CLASS
"#);
    let f = find_func(&m, "Empty");
    assert_eq!(f.kind, PouKind::Class);
}

#[test]
fn compile_class_with_vars() {
    let m = compile_ok(r#"
CLASS Counter
VAR
    count : INT := 0;
    name : STRING;
END_VAR
END_CLASS
"#);
    let f = find_func(&m, "Counter");
    assert_eq!(f.kind, PouKind::Class);
    assert!(f.locals.slots.len() >= 2);
}

#[test]
fn compile_class_with_method() {
    let m = compile_ok(r#"
CLASS Calculator
METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
END_CLASS
"#);
    let class_func = find_func(&m, "Calculator");
    assert_eq!(class_func.kind, PouKind::Class);

    let method_func = find_func(&m, "Calculator.Add");
    assert_eq!(method_func.kind, PouKind::Method);
    // Method should have local slots for a, b, and Add (return var)
    assert!(method_func.locals.slots.len() >= 3);
}

#[test]
fn compile_class_multiple_methods() {
    let m = compile_ok(r#"
CLASS Math
METHOD Add : INT
VAR_INPUT a : INT; b : INT; END_VAR
    Add := a + b;
END_METHOD
METHOD Sub : INT
VAR_INPUT a : INT; b : INT; END_VAR
    Sub := a - b;
END_METHOD
METHOD Mul : INT
VAR_INPUT a : INT; b : INT; END_VAR
    Mul := a * b;
END_METHOD
END_CLASS
"#);
    find_func(&m, "Math");
    find_func(&m, "Math.Add");
    find_func(&m, "Math.Sub");
    find_func(&m, "Math.Mul");
    assert_eq!(
        m.functions.iter().filter(|f| f.name.starts_with("Math")).count(),
        4 // class + 3 methods
    );
}

// =============================================================================
// Abstract methods don't generate code
// =============================================================================

#[test]
fn compile_abstract_method_no_code() {
    let m = compile_ok(r#"
ABSTRACT CLASS Shape
ABSTRACT METHOD Area : REAL
END_METHOD
METHOD Name : INT
    Name := 0;
END_METHOD
END_CLASS
"#);
    find_func(&m, "Shape");
    find_func(&m, "Shape.Name");
    // Abstract method should NOT be compiled
    assert!(
        !m.functions.iter().any(|f| f.name == "Shape.Area"),
        "Abstract methods should not be compiled"
    );
}

// =============================================================================
// Inheritance compilation
// =============================================================================

#[test]
fn compile_class_with_inheritance() {
    let m = compile_ok(r#"
CLASS Base
VAR
    x : INT;
END_VAR
METHOD GetX : INT
    GetX := x;
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
VAR
    y : INT;
END_VAR
METHOD GetY : INT
    GetY := y;
END_METHOD
END_CLASS
"#);
    find_func(&m, "Base");
    find_func(&m, "Base.GetX");
    find_func(&m, "Derived");
    find_func(&m, "Derived.GetY");
}

// =============================================================================
// Interface compilation
// =============================================================================

#[test]
fn compile_interface_no_code() {
    let m = compile_ok(r#"
INTERFACE ICountable
METHOD GetCount : INT
END_METHOD
END_INTERFACE
"#);
    // Interfaces should not produce any function entries
    assert!(
        !m.functions.iter().any(|f| f.name.contains("ICountable")),
        "Interfaces should not produce function entries"
    );
}

// =============================================================================
// Class alongside other POUs
// =============================================================================

#[test]
fn compile_class_and_program() {
    let m = compile_ok(r#"
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

PROGRAM Main
VAR
    c : Counter;
    val : INT;
END_VAR
    val := 0;
END_PROGRAM
"#);
    find_func(&m, "Counter");
    find_func(&m, "Counter.Increment");
    find_func(&m, "Counter.GetCount");
    find_func(&m, "Main");
}

#[test]
fn compile_class_method_has_instructions() {
    let m = compile_ok(r#"
CLASS Adder
METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
END_CLASS
"#);
    let method = find_func(&m, "Adder.Add");
    assert!(
        !method.instructions.is_empty(),
        "Method should have instructions"
    );
}

// =============================================================================
// Method with control flow
// =============================================================================

#[test]
fn compile_method_with_control_flow() {
    let m = compile_ok(r#"
CLASS Logic
METHOD Process : INT
VAR_INPUT
    x : INT;
END_VAR
VAR
    result : INT;
END_VAR
    IF x > 0 THEN
        result := x * 2;
    ELSE
        result := 0;
    END_IF;
    Process := result;
END_METHOD
END_CLASS
"#);
    let method = find_func(&m, "Logic.Process");
    assert!(method.instructions.len() > 5, "Method with IF should have multiple instructions");
}

// =============================================================================
// Class with multiple var blocks
// =============================================================================

#[test]
fn compile_class_var_blocks() {
    let m = compile_ok(r#"
CLASS FullClass
VAR_INPUT
    in1 : INT;
    in2 : BOOL;
END_VAR
VAR_OUTPUT
    out1 : REAL;
END_VAR
VAR
    local1 : INT;
    local2 : STRING;
END_VAR
END_CLASS
"#);
    let f = find_func(&m, "FullClass");
    assert_eq!(f.locals.slots.len(), 5);
}

// =============================================================================
// Void method compilation
// =============================================================================

#[test]
fn compile_void_method() {
    let m = compile_ok(r#"
CLASS Worker
METHOD DoWork
VAR
    x : INT;
END_VAR
    x := 42;
END_METHOD
END_CLASS
"#);
    let method = find_func(&m, "Worker.DoWork");
    // Void method should end with RetVoid
    let last = method.instructions.last().unwrap();
    assert!(
        matches!(last, st_ir::Instruction::RetVoid),
        "Void method should end with RetVoid"
    );
}

#[test]
fn compile_return_method() {
    let m = compile_ok(r#"
CLASS Getter
METHOD Get : INT
    Get := 42;
END_METHOD
END_CLASS
"#);
    let method = find_func(&m, "Getter.Get");
    // Return method should end with Ret
    let last = method.instructions.last().unwrap();
    assert!(
        matches!(last, st_ir::Instruction::Ret(_)),
        "Return method should end with Ret, got {last:?}"
    );
}

#[test]
fn compile_3level_inheritance_var_layout() {
    let m = compile_ok(r#"
CLASS A
VAR aVal : INT := 1; END_VAR
METHOD GetA : INT GetA := aVal; END_METHOD
END_CLASS
CLASS B EXTENDS A
VAR bVal : INT := 10; END_VAR
METHOD GetAB : INT GetAB := aVal + bVal; END_METHOD
END_CLASS
CLASS C EXTENDS B
VAR cVal : INT := 100; END_VAR
METHOD GetABC : INT GetABC := aVal + bVal + cVal; END_METHOD
END_CLASS
PROGRAM Main
VAR obj : C; END_VAR
    obj.GetABC();
END_PROGRAM
"#);
    // C's class function should have all 3 inherited vars
    let c_func = find_func(&m, "C");
    assert_eq!(c_func.locals.slots.len(), 3, "C should have 3 vars (aVal, bVal, cVal)");

    // C.GetABC should also have all 3 class vars + return var
    let abc = find_func(&m, "C.GetABC");
    assert_eq!(abc.locals.slots.len(), 4, "GetABC should have aVal, bVal, cVal, GetABC");

    // B.GetAB should have A's + B's vars + return var
    let ab = find_func(&m, "B.GetAB");
    assert_eq!(ab.locals.slots.len(), 3, "GetAB should have aVal, bVal, GetAB");

    // A.GetA should have just A's var + return var
    let a = find_func(&m, "A.GetA");
    assert_eq!(a.locals.slots.len(), 2, "GetA should have aVal, GetA");
}
