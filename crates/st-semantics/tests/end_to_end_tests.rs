//! End-to-end tests: full programs parsed → analyzed, covering realistic scenarios.

mod test_helpers;
use st_semantics::diagnostic::{DiagnosticCode, Severity};
use test_helpers::*;

// =============================================================================
// Realistic PLC programs — should pass cleanly
// =============================================================================

#[test]
fn pid_controller_program() {
    assert_no_errors(
        r#"
FUNCTION_BLOCK PID
VAR_INPUT
    setpoint : REAL;
    actual : REAL;
    kp : REAL;
    ki : REAL;
    kd : REAL;
END_VAR
VAR_OUTPUT
    output : REAL;
END_VAR
VAR
    error : REAL := 0.0;
    prev_error : REAL := 0.0;
    integral : REAL := 0.0;
    derivative : REAL := 0.0;
END_VAR
    error := setpoint - actual;
    integral := integral + error;
    derivative := error - prev_error;
    output := kp * error + ki * integral + kd * derivative;
    prev_error := error;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    pid1 : PID;
    sensor : REAL := 25.0;
    target : REAL := 50.0;
    valve_pos : REAL := 0.0;
END_VAR
    pid1(setpoint := target, actual := sensor, kp := 1.0, ki := 0.1, kd := 0.05);
    valve_pos := pid1.output;
END_PROGRAM
"#,
    );
}

#[test]
fn state_machine_program() {
    assert_no_errors(
        r#"
PROGRAM StateMachine
VAR
    state : INT := 0;
    timer_count : INT := 0;
    output_active : BOOL := FALSE;
    cycle_count : INT := 0;
END_VAR
    cycle_count := cycle_count + 1;

    CASE state OF
        0:
            output_active := FALSE;
            timer_count := 0;
            IF cycle_count > 100 THEN
                state := 1;
            END_IF;
        1:
            output_active := TRUE;
            timer_count := timer_count + 1;
            IF timer_count > 50 THEN
                state := 2;
            END_IF;
        2:
            output_active := FALSE;
            state := 0;
            cycle_count := 0;
    END_CASE;
END_PROGRAM
"#,
    );
}

#[test]
fn multi_function_with_types() {
    assert_no_errors(
        r#"
TYPE
    Measurement : STRUCT
        value : REAL := 0.0;
        valid : BOOL := FALSE;
    END_STRUCT;
END_TYPE

FUNCTION Clamp : REAL
VAR_INPUT
    val : REAL;
    lo : REAL;
    hi : REAL;
END_VAR
    IF val < lo THEN
        Clamp := lo;
    ELSIF val > hi THEN
        Clamp := hi;
    ELSE
        Clamp := val;
    END_IF;
END_FUNCTION

FUNCTION Scale : REAL
VAR_INPUT
    raw : REAL;
    in_lo : REAL;
    in_hi : REAL;
    out_lo : REAL;
    out_hi : REAL;
END_VAR
VAR
    ratio : REAL := 0.0;
END_VAR
    ratio := (raw - in_lo) / (in_hi - in_lo);
    Scale := out_lo + ratio * (out_hi - out_lo);
END_FUNCTION

PROGRAM Main
VAR
    raw_input : REAL := 512.0;
    scaled : REAL := 0.0;
    clamped : REAL := 0.0;
    m : Measurement;
END_VAR
    scaled := Scale(raw := raw_input, in_lo := 0.0, in_hi := 1023.0, out_lo := 0.0, out_hi := 100.0);
    clamped := Clamp(val := scaled, lo := 0.0, hi := 100.0);
    m.value := clamped;
    m.valid := TRUE;
END_PROGRAM
"#,
    );
}

#[test]
fn array_processing_program() {
    assert_no_errors(
        r#"
FUNCTION FindMax : INT
VAR_INPUT
    data : ARRAY[1..10] OF INT;
    count : INT;
END_VAR
VAR
    i : INT;
    max : INT := 0;
END_VAR
    max := data[1];
    FOR i := 2 TO count DO
        IF data[i] > max THEN
            max := data[i];
        END_IF;
    END_FOR;
    FindMax := max;
END_FUNCTION

PROGRAM Main
VAR
    values : ARRAY[1..10] OF INT;
    i : INT;
    max_val : INT := 0;
END_VAR
    FOR i := 1 TO 10 DO
        values[i] := i * 3;
    END_FOR;
    max_val := FindMax(data := values, count := 10);
END_PROGRAM
"#,
    );
}

#[test]
fn nested_control_flow() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    i : INT;
    j : INT;
    sum : INT := 0;
    found : BOOL := FALSE;
END_VAR
    FOR i := 1 TO 10 DO
        FOR j := 1 TO 10 DO
            IF i * j > 50 THEN
                found := TRUE;
                EXIT;
            END_IF;
            sum := sum + i * j;
        END_FOR;
        IF found THEN
            EXIT;
        END_IF;
    END_FOR;

    WHILE sum > 0 DO
        sum := sum - 1;
        IF sum < 10 THEN
            EXIT;
        END_IF;
    END_WHILE;

    REPEAT
        sum := sum + 1;
    UNTIL sum >= 100
    END_REPEAT;
END_PROGRAM
"#,
    );
}

#[test]
fn enum_type_in_case() {
    assert_no_errors(
        r#"
TYPE
    TrafficLight : (Red, Yellow, Green);
END_TYPE

PROGRAM Main
VAR
    light : INT := 0;
    output : BOOL := FALSE;
END_VAR
    CASE light OF
        0:
            output := FALSE;
        1:
            output := FALSE;
        2:
            output := TRUE;
    END_CASE;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Programs that should produce specific errors
// =============================================================================

#[test]
fn multiple_errors_in_one_program() {
    let result = analyze(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    b : BOOL := TRUE;
END_VAR
    x := undeclared1;
    x := undeclared2;
    x := b;
    IF x THEN
        x := 1;
    END_IF;
END_PROGRAM
"#,
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // Should have at least: 2x UndeclaredVariable, 1x TypeMismatchAssignment, 1x TypeMismatchCondition
    assert!(
        errors.len() >= 4,
        "Expected at least 4 errors, got {}: {:?}",
        errors.len(),
        errors.iter().map(|e| format!("{:?}: {}", e.code, e.message)).collect::<Vec<_>>()
    );

    let codes: Vec<_> = errors.iter().map(|e| e.code).collect();
    assert!(codes.iter().filter(|c| **c == DiagnosticCode::UndeclaredVariable).count() >= 2);
    assert!(codes.contains(&DiagnosticCode::TypeMismatchAssignment));
    assert!(codes.contains(&DiagnosticCode::TypeMismatchCondition));
}

#[test]
fn mixed_errors_and_warnings() {
    let result = analyze(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    unused_var : INT := 0;
END_VAR
    x := undeclared;
END_PROGRAM
"#,
    );
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    let warnings: Vec<_> = result.diagnostics.iter().filter(|d| d.severity == Severity::Warning).collect();

    assert!(!errors.is_empty(), "Expected at least one error");
    assert!(!warnings.is_empty(), "Expected at least one warning");
}

// =============================================================================
// Edge cases
// =============================================================================

#[test]
fn empty_program_body() {
    assert_no_errors(
        r#"
PROGRAM Empty
VAR
    x : INT := 0;
END_VAR
    x := 0;
END_PROGRAM
"#,
    );
}

#[test]
fn program_minimal() {
    assert_no_errors(
        r#"
PROGRAM Minimal
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
"#,
    );
}

#[test]
fn deeply_nested_expressions() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := ((((1 + 2) * 3) - 4) / 5) MOD 6;
END_PROGRAM
"#,
    );
}

#[test]
fn chained_assignments() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : INT := 0;
    b : INT := 0;
    c : INT := 0;
END_VAR
    a := 1;
    b := a;
    c := b;
    a := c;
END_PROGRAM
"#,
    );
}

#[test]
fn function_calling_function() {
    assert_no_errors(
        r#"
FUNCTION Inner : INT
VAR_INPUT
    x : INT;
END_VAR
    Inner := x * 2;
END_FUNCTION

FUNCTION Outer : INT
VAR_INPUT
    y : INT;
END_VAR
    Outer := Inner(x := y) + 1;
END_FUNCTION

PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := Outer(y := 5);
END_PROGRAM
"#,
    );
}

#[test]
fn fb_calling_function() {
    assert_no_errors(
        r#"
FUNCTION Helper : INT
VAR_INPUT
    val : INT;
END_VAR
    Helper := val + 1;
END_FUNCTION

FUNCTION_BLOCK Processor
VAR_INPUT
    input : INT;
END_VAR
VAR_OUTPUT
    output : INT;
END_VAR
    output := Helper(val := input);
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    proc : Processor;
    result : INT := 0;
END_VAR
    proc(input := 42);
    result := proc.output;
END_PROGRAM
"#,
    );
}

#[test]
fn all_elementary_type_variables() {
    assert_no_errors(
        r#"
PROGRAM TypeShowcase
VAR
    v_bool : BOOL := TRUE;
    v_int : INT := 0;
    v_dint : DINT := 0;
    v_lint : LINT := 0;
    v_uint : UINT := 0;
    v_udint : UDINT := 0;
    v_ulint : ULINT := 0;
    v_real : REAL := 0.0;
    v_lreal : LREAL := 0.0;
    v_byte : BYTE;
    v_word : WORD;
    v_dword : DWORD;
    v_lword : LWORD;
END_VAR
    v_bool := NOT v_bool;
    v_int := v_int + 1;
    v_dint := v_dint + 1;
    v_lint := v_lint + 1;
    v_uint := v_uint + 1;
    v_udint := v_udint + 1;
    v_ulint := v_ulint + 1;
    v_real := v_real + 1.0;
    v_lreal := v_lreal + 1.0;
    v_byte := v_byte AND v_byte;
    v_word := v_word OR v_word;
    v_dword := v_dword XOR v_dword;
    v_lword := NOT v_lword;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Using st_semantics::check convenience function (full parse+analyze pipeline)
// =============================================================================

#[test]
fn check_convenience_clean_program() {
    let result = st_semantics::check(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
END_PROGRAM
"#,
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "Expected no errors: {errors:?}");
}

#[test]
fn check_convenience_with_errors() {
    let result = st_semantics::check(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := undeclared;
END_PROGRAM
"#,
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "Expected errors for undeclared variable");
}
