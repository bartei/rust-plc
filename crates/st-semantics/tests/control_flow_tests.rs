//! Tests for control flow analysis.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// FOR loop
// =============================================================================

#[test]
fn for_with_int_variable() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT;
    sum : INT := 0;
END_VAR
    FOR i := 1 TO 10 DO
        sum := sum + i;
    END_FOR;
END_PROGRAM
"#,
    );
}

#[test]
fn for_with_step() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT;
    sum : INT := 0;
END_VAR
    FOR i := 0 TO 100 BY 5 DO
        sum := sum + i;
    END_FOR;
END_PROGRAM
"#,
    );
}

#[test]
fn for_with_real_variable_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    r : REAL := 0.0;
    sum : REAL := 0.0;
END_VAR
    FOR r := 1.0 TO 10.0 DO
        sum := sum + r;
    END_FOR;
END_PROGRAM
"#,
        &[DiagnosticCode::ForVariableNotInteger],
    );
}

#[test]
fn for_with_bool_variable_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    b : BOOL := FALSE;
    x : INT := 0;
END_VAR
    FOR b := 1 TO 10 DO
        x := x + 1;
    END_FOR;
END_PROGRAM
"#,
        &[DiagnosticCode::ForVariableNotInteger],
    );
}

#[test]
fn for_with_undeclared_variable() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    sum : INT := 0;
END_VAR
    FOR missing := 1 TO 10 DO
        sum := sum + 1;
    END_FOR;
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredVariable],
    );
}

#[test]
fn for_bounds_must_be_integer() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    i : INT;
    x : INT := 0;
END_VAR
    FOR i := 1.0 TO 10.0 DO
        x := x + 1;
    END_FOR;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatch],
    );
}

// =============================================================================
// EXIT statement
// =============================================================================

#[test]
fn exit_inside_for_loop() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT;
    x : INT := 0;
END_VAR
    FOR i := 1 TO 100 DO
        IF i > 50 THEN
            EXIT;
        END_IF;
        x := x + i;
    END_FOR;
END_PROGRAM
"#,
    );
}

#[test]
fn exit_inside_while_loop() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 100;
END_VAR
    WHILE TRUE DO
        x := x - 1;
        IF x < 0 THEN
            EXIT;
        END_IF;
    END_WHILE;
END_PROGRAM
"#,
    );
}

#[test]
fn exit_inside_repeat_loop() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    REPEAT
        x := x + 1;
        IF x > 50 THEN
            EXIT;
        END_IF;
    UNTIL x >= 100
    END_REPEAT;
END_PROGRAM
"#,
    );
}

#[test]
fn exit_outside_loop_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
    EXIT;
END_PROGRAM
"#,
        &[DiagnosticCode::ExitOutsideLoop],
    );
}

// =============================================================================
// CASE statement
// =============================================================================

#[test]
fn case_matching_types() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    mode : INT := 1;
    x : INT := 0;
END_VAR
    CASE mode OF
        1:
            x := 10;
        2:
            x := 20;
        3:
            x := 30;
    END_CASE;
END_PROGRAM
"#,
    );
}

#[test]
fn case_with_range_selectors() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    val : INT := 50;
    category : INT := 0;
END_VAR
    CASE val OF
        1..10:
            category := 1;
        11..100:
            category := 2;
    END_CASE;
END_PROGRAM
"#,
    );
}

#[test]
fn case_with_else() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    mode : INT := 1;
    x : INT := 0;
END_VAR
    CASE mode OF
        1:
            x := 10;
    ELSE
        x := 0;
    END_CASE;
END_PROGRAM
"#,
    );
}

#[test]
fn case_selector_type_mismatch() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    mode : INT := 1;
    x : INT := 0;
END_VAR
    CASE mode OF
        TRUE:
            x := 10;
    END_CASE;
END_PROGRAM
"#,
        &[DiagnosticCode::CaseSelectorTypeMismatch],
    );
}

// =============================================================================
// Dead code detection
// =============================================================================

#[test]
fn dead_code_after_return() {
    assert_has_warnings(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    x : INT;
END_VAR
    Foo := x;
    RETURN;
    Foo := x + 1;
END_FUNCTION
"#,
        &[DiagnosticCode::DeadCode],
    );
}

#[test]
fn no_dead_code_without_return() {
    assert_no_warning(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    x : INT;
END_VAR
    Foo := x;
    Foo := Foo + 1;
END_FUNCTION
"#,
        DiagnosticCode::DeadCode,
    );
}
