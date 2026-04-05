//! Tests for scope resolution and symbol table.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// Variable resolution — success cases
// =============================================================================

#[test]
fn resolve_local_var() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
END_PROGRAM
"#,
    );
}

#[test]
fn resolve_var_input() {
    assert_no_errors(
        r#"
FUNCTION_BLOCK FB1
VAR_INPUT
    enable : BOOL;
END_VAR
VAR
    state : INT := 0;
END_VAR
    IF enable THEN
        state := 1;
    END_IF;
END_FUNCTION_BLOCK
"#,
    );
}

#[test]
fn resolve_var_output() {
    assert_no_errors(
        r#"
FUNCTION_BLOCK FB1
VAR_OUTPUT
    result : INT;
END_VAR
    result := 42;
END_FUNCTION_BLOCK
"#,
    );
}

#[test]
fn resolve_across_multiple_var_blocks() {
    assert_no_errors(
        r#"
FUNCTION_BLOCK FB1
VAR_INPUT
    a : INT;
END_VAR
VAR_OUTPUT
    b : INT;
END_VAR
VAR
    temp : INT := 0;
END_VAR
    temp := a;
    b := temp;
END_FUNCTION_BLOCK
"#,
    );
}

#[test]
fn resolve_global_var() {
    assert_no_errors(
        r#"
VAR_GLOBAL
    gCounter : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    gCounter := gCounter + 1;
    x := gCounter;
END_PROGRAM
"#,
    );
}

#[test]
fn resolve_function_return_variable() {
    assert_no_errors(
        r#"
FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION
"#,
    );
}

#[test]
fn resolve_forward_reference_to_function() {
    // Main references Add which is declared after it
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := Add(a := 1, b := 2);
END_PROGRAM

FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION
"#,
    );
}

#[test]
fn resolve_forward_reference_to_fb() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    cnt : Counter;
END_VAR
    cnt(reset := FALSE);
END_PROGRAM

FUNCTION_BLOCK Counter
VAR_INPUT
    reset : BOOL;
END_VAR
VAR
    val : INT := 0;
END_VAR
    IF reset THEN
        val := 0;
    ELSE
        val := val + 1;
    END_IF;
END_FUNCTION_BLOCK
"#,
    );
}

#[test]
fn case_insensitive_resolution() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    MyVar : INT := 0;
END_VAR
    myvar := MYVAR + 1;
END_PROGRAM
"#,
    );
}

#[test]
fn multiple_names_in_one_declaration() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a, b, c : INT := 0;
END_VAR
    a := 1;
    b := 2;
    c := a + b;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Variable resolution — failure cases
// =============================================================================

#[test]
fn undeclared_variable() {
    assert_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := y;
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

#[test]
fn undeclared_variable_in_expression() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + unknown_var;
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

#[test]
fn undeclared_variable_as_assignment_target() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    nonexistent := x;
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

#[test]
fn undeclared_variable_in_condition() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    IF undeclared THEN
        x := 1;
    END_IF;
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

#[test]
fn undeclared_variable_in_for_loop() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    FOR undeclared := 1 TO 10 DO
        x := x + 1;
    END_FOR;
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

// =============================================================================
// Duplicate declarations
// =============================================================================

#[test]
fn duplicate_variable_in_same_block() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT;
    x : REAL;
END_VAR
    x := 1;
END_PROGRAM
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

#[test]
fn duplicate_program_name() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM

PROGRAM Main
VAR
    y : INT := 0;
END_VAR
    y := 2;
END_PROGRAM
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

#[test]
fn duplicate_function_name() {
    assert_has_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    x : INT;
END_VAR
    Foo := x;
END_FUNCTION

FUNCTION Foo : REAL
VAR_INPUT
    y : REAL;
END_VAR
    Foo := y;
END_FUNCTION
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

#[test]
fn duplicate_type_name() {
    assert_has_errors(
        r#"
TYPE
    Color : (Red, Green, Blue);
END_TYPE
TYPE
    Color : (Cyan, Magenta, Yellow);
END_TYPE

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

// =============================================================================
// Shadowing
// =============================================================================

#[test]
fn shadowed_global_variable() {
    assert_has_warnings(
        r#"
VAR_GLOBAL
    x : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
"#,
        &[DiagnosticCode::ShadowedVariable],
    );
}

// =============================================================================
// POU resolution
// =============================================================================

#[test]
fn undeclared_function_call() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := NonexistentFunc(a := 1);
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredPou],
    );
}

#[test]
fn call_non_callable() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x(a := 1);
END_PROGRAM
"#,
        &[DiagnosticCode::NotCallable],
    );
}
