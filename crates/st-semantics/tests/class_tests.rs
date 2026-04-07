//! Semantic analysis tests for IEC 61131-3 OOP extensions (Phase 12).

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// Basic class declarations
// =============================================================================

#[test]
fn class_basic_declaration() {
    assert_no_errors(r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
METHOD Increment
    count := count + 1;
END_METHOD
END_CLASS
"#);
}

#[test]
fn class_with_var_input_output() {
    assert_no_errors(r#"
CLASS Adder
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
VAR_OUTPUT
    result : INT;
END_VAR
METHOD Execute
    result := a + b;
END_METHOD
END_CLASS
"#);
}

#[test]
fn class_method_with_return_type() {
    assert_no_errors(r#"
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
}

#[test]
fn class_method_local_vars() {
    assert_no_errors(r#"
CLASS Processor
METHOD Compute : REAL
VAR_INPUT
    x : REAL;
END_VAR
VAR
    temp : REAL;
END_VAR
    temp := x * 2.0;
    Compute := temp + 1.0;
END_METHOD
END_CLASS
"#);
}

#[test]
fn class_multiple_methods() {
    assert_no_errors(r#"
CLASS Shape
METHOD GetArea : REAL
    GetArea := 0.0;
END_METHOD
METHOD GetPerimeter : REAL
    GetPerimeter := 0.0;
END_METHOD
END_CLASS
"#);
}

// =============================================================================
// Duplicate declarations
// =============================================================================

#[test]
fn class_duplicate_name_error() {
    assert_has_errors(
        r#"
CLASS Foo
END_CLASS
CLASS Foo
END_CLASS
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

// =============================================================================
// Inheritance — EXTENDS
// =============================================================================

#[test]
fn class_extends_valid() {
    assert_no_errors(r#"
CLASS Base
VAR
    x : INT;
END_VAR
END_CLASS

CLASS Derived EXTENDS Base
VAR
    y : INT;
END_VAR
END_CLASS
"#);
}

#[test]
fn class_extends_undeclared_base() {
    assert_has_errors(
        r#"
CLASS Derived EXTENDS NonExistent
END_CLASS
"#,
        &[DiagnosticCode::UndeclaredType],
    );
}

#[test]
fn class_extends_final_class() {
    assert_has_errors(
        r#"
FINAL CLASS Sealed
END_CLASS

CLASS Derived EXTENDS Sealed
END_CLASS
"#,
        &[DiagnosticCode::CannotExtendFinalClass],
    );
}

// =============================================================================
// ABSTRACT classes
// =============================================================================

#[test]
fn abstract_class_with_abstract_method() {
    assert_no_errors(r#"
ABSTRACT CLASS Shape
ABSTRACT METHOD Area : REAL
END_METHOD
END_CLASS
"#);
}

#[test]
fn abstract_method_in_non_abstract_class() {
    assert_has_errors(
        r#"
CLASS BadClass
ABSTRACT METHOD DoSomething
END_METHOD
END_CLASS
"#,
        &[DiagnosticCode::AbstractMethodInNonAbstractClass],
    );
}

// =============================================================================
// Interfaces
// =============================================================================

#[test]
fn interface_basic_declaration() {
    assert_no_errors(r#"
INTERFACE ICountable
METHOD GetCount : INT
END_METHOD
END_INTERFACE
"#);
}

#[test]
fn interface_extends_valid() {
    assert_no_errors(r#"
INTERFACE IBase
METHOD GetName : INT
END_METHOD
END_INTERFACE

INTERFACE IDerived EXTENDS IBase
METHOD GetId : INT
END_METHOD
END_INTERFACE
"#);
}

#[test]
fn interface_extends_undeclared() {
    assert_has_errors(
        r#"
INTERFACE IBad EXTENDS INonExistent
METHOD Foo
END_METHOD
END_INTERFACE
"#,
        &[DiagnosticCode::UndeclaredType],
    );
}

#[test]
fn interface_duplicate_name() {
    assert_has_errors(
        r#"
INTERFACE IFoo
END_INTERFACE
INTERFACE IFoo
END_INTERFACE
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

// =============================================================================
// IMPLEMENTS — Interface conformance
// =============================================================================

#[test]
fn class_implements_all_methods() {
    assert_no_errors(r#"
INTERFACE IGreeter
METHOD Greet : INT
END_METHOD
END_INTERFACE

CLASS Greeter IMPLEMENTS IGreeter
METHOD Greet : INT
    Greet := 42;
END_METHOD
END_CLASS
"#);
}

#[test]
fn class_missing_interface_method() {
    assert_has_errors(
        r#"
INTERFACE ICountable
METHOD GetCount : INT
END_METHOD
METHOD Reset
END_METHOD
END_INTERFACE

CLASS BadCounter IMPLEMENTS ICountable
METHOD GetCount : INT
    GetCount := 0;
END_METHOD
// Missing: Reset
END_CLASS
"#,
        &[DiagnosticCode::InterfaceNotImplemented],
    );
}

#[test]
fn class_implements_undeclared_interface() {
    assert_has_errors(
        r#"
CLASS BadClass IMPLEMENTS INonExistent
END_CLASS
"#,
        &[DiagnosticCode::UndeclaredType],
    );
}

#[test]
fn class_implements_multiple_interfaces() {
    assert_no_errors(r#"
INTERFACE IFoo
METHOD Foo
END_METHOD
END_INTERFACE

INTERFACE IBar
METHOD Bar
END_METHOD
END_INTERFACE

CLASS FooBar IMPLEMENTS IFoo, IBar
METHOD Foo
END_METHOD
METHOD Bar
END_METHOD
END_CLASS
"#);
}

// =============================================================================
// OVERRIDE
// =============================================================================

#[test]
fn method_override_valid() {
    assert_no_errors(r#"
CLASS Base
METHOD Process
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
OVERRIDE METHOD Process
END_METHOD
END_CLASS
"#);
}

#[test]
fn method_override_no_base_class() {
    assert_has_errors(
        r#"
CLASS NoBase
OVERRIDE METHOD Foo
END_METHOD
END_CLASS
"#,
        &[DiagnosticCode::InvalidOverride],
    );
}

#[test]
fn method_override_no_matching_base_method() {
    assert_has_errors(
        r#"
CLASS Base
METHOD Something
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
OVERRIDE METHOD NonExistent
END_METHOD
END_CLASS
"#,
        &[DiagnosticCode::InvalidOverride],
    );
}

#[test]
fn method_override_final_method() {
    assert_has_errors(
        r#"
CLASS Base
FINAL METHOD Locked
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
OVERRIDE METHOD Locked
END_METHOD
END_CLASS
"#,
        &[DiagnosticCode::CannotOverrideFinalMethod],
    );
}

// =============================================================================
// Access specifiers
// =============================================================================

#[test]
fn method_access_specifiers_parse() {
    assert_no_errors(r#"
CLASS MyClass
PUBLIC METHOD PubMethod
END_METHOD
PRIVATE METHOD PrivMethod
END_METHOD
PROTECTED METHOD ProtMethod
END_METHOD
INTERNAL METHOD IntMethod
END_METHOD
END_CLASS
"#);
}

// =============================================================================
// THIS / SUPER context validation
// =============================================================================

#[test]
fn this_valid_inside_method() {
    // THIS used in class context should not produce errors about THIS itself
    // (it will have other type resolution issues but not InvalidThisContext)
    let result = analyze(r#"
CLASS MyClass
VAR
    x : INT;
END_VAR
METHOD SetX
VAR_INPUT
    val : INT;
END_VAR
    x := val;
END_METHOD
END_CLASS
"#);
    let this_errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.code == DiagnosticCode::InvalidThisContext)
        .collect();
    assert!(this_errors.is_empty(), "Unexpected InvalidThisContext errors");
}

#[test]
fn super_valid_inside_derived_method() {
    let result = analyze(r#"
CLASS Base
METHOD DoWork
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
METHOD DoWork
END_METHOD
END_CLASS
"#);
    let super_errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.code == DiagnosticCode::InvalidSuperContext)
        .collect();
    assert!(super_errors.is_empty(), "Unexpected InvalidSuperContext errors");
}

// =============================================================================
// Properties
// =============================================================================

#[test]
fn property_get_set() {
    assert_no_errors(r#"
CLASS MyClass
VAR
    _value : INT;
END_VAR
PROPERTY Value : INT
GET
    Value := _value;
END_GET
SET
    _value := Value;
END_SET
END_PROPERTY
END_CLASS
"#);
}

#[test]
fn property_get_only() {
    assert_no_errors(r#"
CLASS ReadOnly
VAR
    _data : REAL;
END_VAR
PROPERTY Data : REAL
GET
    Data := _data;
END_GET
END_PROPERTY
END_CLASS
"#);
}

// =============================================================================
// Class instance usage
// =============================================================================

#[test]
fn class_instance_in_program() {
    assert_no_errors(r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
END_CLASS

PROGRAM Main
VAR
    c : Counter;
    x : INT;
END_VAR
    x := 0;
END_PROGRAM
"#);
}

// =============================================================================
// Type checking inside methods
// =============================================================================

#[test]
fn method_type_mismatch() {
    assert_has_errors(
        r#"
CLASS TypeTest
METHOD Bad
VAR
    x : INT;
    s : STRING;
END_VAR
    x := s;
END_METHOD
END_CLASS
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

#[test]
fn method_undeclared_variable() {
    assert_has_errors(
        r#"
CLASS VarTest
METHOD Bad
    undeclared := 42;
END_METHOD
END_CLASS
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

// =============================================================================
// Control flow inside methods
// =============================================================================

#[test]
fn method_with_if_for_while() {
    assert_no_errors(r#"
CLASS Logic
METHOD Process
VAR
    i : INT;
    x : INT := 0;
    flag : BOOL := TRUE;
END_VAR
    IF flag THEN
        x := 1;
    ELSE
        x := 2;
    END_IF;

    FOR i := 0 TO 10 DO
        x := x + i;
    END_FOR;

    WHILE x > 0 DO
        x := x - 1;
    END_WHILE;
END_METHOD
END_CLASS
"#);
}

// =============================================================================
// Complex inheritance scenarios
// =============================================================================

#[test]
fn multi_level_inheritance() {
    assert_no_errors(r#"
CLASS Animal
VAR
    name : INT;
END_VAR
END_CLASS

CLASS Mammal EXTENDS Animal
VAR
    legs : INT;
END_VAR
END_CLASS

CLASS Dog EXTENDS Mammal
VAR
    breed : INT;
END_VAR
END_CLASS
"#);
}

#[test]
fn interface_and_class_hierarchy() {
    assert_no_errors(r#"
INTERFACE IMovable
METHOD Move
END_METHOD
END_INTERFACE

INTERFACE IDrawable
METHOD Draw
END_METHOD
END_INTERFACE

ABSTRACT CLASS Shape
ABSTRACT METHOD Area : REAL
END_METHOD
END_CLASS

CLASS Circle EXTENDS Shape IMPLEMENTS IMovable, IDrawable
METHOD Area : REAL
    Area := 3.14;
END_METHOD
METHOD Move
END_METHOD
METHOD Draw
END_METHOD
END_CLASS
"#);
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn empty_class_is_valid() {
    assert_no_errors(r#"
CLASS Empty
END_CLASS
"#);
}

#[test]
fn class_method_can_access_class_vars() {
    assert_no_errors(r#"
CLASS DataStore
VAR
    value : INT := 0;
END_VAR
METHOD GetValue : INT
    GetValue := value;
END_METHOD
METHOD SetValue
VAR_INPUT
    newVal : INT;
END_VAR
    value := newVal;
END_METHOD
END_CLASS
"#);
}

#[test]
fn class_var_blocks_all_kinds() {
    assert_no_errors(r#"
CLASS FullVars
VAR_INPUT
    in1 : INT;
END_VAR
VAR_OUTPUT
    out1 : BOOL;
END_VAR
VAR
    local1 : REAL;
END_VAR
END_CLASS
"#);
}

// =============================================================================
// Method calls on class instances
// =============================================================================

#[test]
fn method_call_on_class_instance() {
    assert_no_errors(r#"
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
    c.Increment();
    val := c.GetCount();
END_PROGRAM
"#);
}

#[test]
fn method_call_with_args() {
    assert_no_errors(r#"
CLASS Adder
METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    calc : Adder;
    result : INT;
END_VAR
    result := calc.Add(a := 3, b := 4);
END_PROGRAM
"#);
}

#[test]
fn method_call_nonexistent_method() {
    assert_has_errors(
        r#"
CLASS Foo
METHOD Bar
END_METHOD
END_CLASS

PROGRAM Main
VAR
    f : Foo;
END_VAR
    f.NonExistent();
END_PROGRAM
"#,
        &[DiagnosticCode::NoSuchField],
    );
}

// =============================================================================
// Inherited method calls
// =============================================================================

#[test]
fn inherited_method_call() {
    assert_no_errors(r#"
CLASS Base
METHOD DoWork
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
METHOD DoExtra
END_METHOD
END_CLASS

PROGRAM Main
VAR
    d : Derived;
END_VAR
    d.DoWork();
    d.DoExtra();
END_PROGRAM
"#);
}

#[test]
fn deep_inherited_method_call() {
    assert_no_errors(r#"
CLASS A
METHOD FromA
END_METHOD
END_CLASS

CLASS B EXTENDS A
METHOD FromB
END_METHOD
END_CLASS

CLASS C EXTENDS B
METHOD FromC
END_METHOD
END_CLASS

PROGRAM Main
VAR
    obj : C;
END_VAR
    obj.FromA();
    obj.FromB();
    obj.FromC();
END_PROGRAM
"#);
}

#[test]
fn overridden_method_call() {
    assert_no_errors(r#"
CLASS Base
METHOD Process : INT
    Process := 0;
END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
OVERRIDE METHOD Process : INT
    Process := 42;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    d : Derived;
    val : INT;
END_VAR
    val := d.Process();
END_PROGRAM
"#);
}
