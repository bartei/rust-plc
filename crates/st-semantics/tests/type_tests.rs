//! Tests for type checking.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// Assignment type compatibility — success
// =============================================================================

#[test]
fn assign_int_to_int() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    y : INT := 0;
END_VAR
    x := y;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_sint_to_int_widening() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    small : SINT;
    big : INT := 0;
END_VAR
    big := small;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_int_to_dint_widening() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT := 0;
    d : DINT := 0;
END_VAR
    d := i;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_int_to_real_widening() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT := 0;
    r : REAL := 0.0;
END_VAR
    r := i;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_real_to_lreal_widening() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    r : REAL := 0.0;
    lr : LREAL := 0.0;
END_VAR
    lr := r;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_literal_int_to_int() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 42;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_literal_real_to_real() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : REAL := 0.0;
END_VAR
    x := 3.14;
END_PROGRAM
"#,
    );
}

#[test]
fn assign_bool_to_bool() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    flag : BOOL := FALSE;
END_VAR
    flag := TRUE;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Assignment type compatibility — failure (narrowing / incompatible)
// =============================================================================

#[test]
fn assign_real_to_int_narrowing() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    i : INT := 0;
    r : REAL := 3.14;
END_VAR
    i := r;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

#[test]
fn assign_bool_to_int() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    b : BOOL := TRUE;
END_VAR
    x := b;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

#[test]
fn assign_int_to_bool() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 1;
    b : BOOL := TRUE;
END_VAR
    b := x;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

#[test]
fn assign_string_to_int() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 'hello';
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

#[test]
fn assign_dint_to_sint_narrowing() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    s : SINT := 0;
    d : DINT := 100000;
END_VAR
    s := d;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

// =============================================================================
// Initial value type checking
// =============================================================================

#[test]
fn initial_value_type_mismatch() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : BOOL := 42;
END_VAR
    x := TRUE;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchAssignment],
    );
}

#[test]
fn initial_value_widening_ok() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : REAL := 42;
END_VAR
    x := x + 1.0;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Arithmetic expression types
// =============================================================================

#[test]
fn arithmetic_int_int() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 1;
    b : INT := 2;
    c : INT := 0;
END_VAR
    c := a + b;
    c := a - b;
    c := a * b;
    c := a / b;
    c := a MOD b;
END_PROGRAM
"#,
    );
}

#[test]
fn arithmetic_real_real() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : REAL := 1.0;
    b : REAL := 2.0;
    c : REAL := 0.0;
END_VAR
    c := a + b;
    c := a - b;
    c := a * b;
    c := a / b;
END_PROGRAM
"#,
    );
}

#[test]
fn arithmetic_int_real_promotion() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT := 1;
    r : REAL := 2.0;
    result : REAL := 0.0;
END_VAR
    result := i + r;
END_PROGRAM
"#,
    );
}

#[test]
fn arithmetic_bool_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    a : BOOL := TRUE;
    b : BOOL := FALSE;
    c : INT := 0;
END_VAR
    c := a + b;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}

#[test]
fn arithmetic_string_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 'hello' + 'world';
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}

#[test]
fn mod_requires_integers() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    a : REAL := 1.0;
    b : REAL := 2.0;
    c : REAL := 0.0;
END_VAR
    c := a MOD b;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}

#[test]
fn power_returns_lreal() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 2;
    b : INT := 3;
    result : LREAL := 0.0;
END_VAR
    result := a ** b;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Boolean / logical expression types
// =============================================================================

#[test]
fn boolean_and_or_xor() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : BOOL := TRUE;
    b : BOOL := FALSE;
    c : BOOL := FALSE;
END_VAR
    c := a AND b;
    c := a OR b;
    c := a XOR b;
END_PROGRAM
"#,
    );
}

#[test]
fn boolean_not() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : BOOL := TRUE;
    b : BOOL := FALSE;
END_VAR
    b := NOT a;
END_PROGRAM
"#,
    );
}

#[test]
fn and_on_integers_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 1;
    b : INT := 2;
    c : INT := 0;
END_VAR
    c := a AND b;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}

#[test]
fn not_on_int_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 1;
    b : INT := 0;
END_VAR
    b := NOT a;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleUnaryOp],
    );
}

#[test]
fn bitwise_on_byte_word() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : BYTE;
    b : BYTE;
    c : BYTE;
END_VAR
    c := a AND b;
    c := a OR b;
    c := a XOR b;
    c := NOT a;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Comparison expression types
// =============================================================================

#[test]
fn comparison_int_int() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 1;
    b : INT := 2;
    result : BOOL := FALSE;
END_VAR
    result := a = b;
    result := a <> b;
    result := a < b;
    result := a > b;
    result := a <= b;
    result := a >= b;
END_PROGRAM
"#,
    );
}

#[test]
fn comparison_result_is_bool() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 1;
    b : INT := 2;
    flag : BOOL := FALSE;
END_VAR
    flag := a > b;
    IF a < b THEN
        flag := TRUE;
    END_IF;
END_PROGRAM
"#,
    );
}

#[test]
fn comparison_mixed_numeric() {
    // INT vs REAL comparison should be OK
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT := 1;
    r : REAL := 2.0;
    flag : BOOL := FALSE;
END_VAR
    flag := i < r;
END_PROGRAM
"#,
    );
}

#[test]
fn comparison_incompatible_types() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    i : INT := 1;
    b : BOOL := TRUE;
    flag : BOOL := FALSE;
END_VAR
    flag := i = b;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}

// =============================================================================
// Unary expression types
// =============================================================================

#[test]
fn negate_int() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 5;
    y : INT := 0;
END_VAR
    y := -x;
END_PROGRAM
"#,
    );
}

#[test]
fn negate_real() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : REAL := 3.14;
    y : REAL := 0.0;
END_VAR
    y := -x;
END_PROGRAM
"#,
    );
}

#[test]
fn negate_bool_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    b : BOOL := TRUE;
    x : INT := 0;
END_VAR
    x := -b;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleUnaryOp],
    );
}

// =============================================================================
// Condition type checking (IF, WHILE, REPEAT)
// =============================================================================

#[test]
fn if_condition_must_be_bool() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 1;
END_VAR
    IF x THEN
        x := 0;
    END_IF;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchCondition],
    );
}

#[test]
fn while_condition_must_be_bool() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 10;
END_VAR
    WHILE x DO
        x := x - 1;
    END_WHILE;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchCondition],
    );
}

#[test]
fn repeat_condition_must_be_bool() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    REPEAT
        x := x + 1;
    UNTIL x
    END_REPEAT;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatchCondition],
    );
}

#[test]
fn if_with_comparison_is_bool() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 1;
END_VAR
    IF x > 0 THEN
        x := 0;
    END_IF;
END_PROGRAM
"#,
    );
}
