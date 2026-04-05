//! Tests for warnings: unused variables, never-assigned, etc.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// Unused variable warnings
// =============================================================================

#[test]
fn unused_local_variable() {
    assert_has_warnings(
        r#"
PROGRAM Main
VAR
    used : INT := 0;
    unused : INT := 0;
END_VAR
    used := 1;
END_PROGRAM
"#,
        &[DiagnosticCode::UnusedVariable],
    );
}

#[test]
fn unused_variable_underscore_prefix_suppressed() {
    // Variables starting with _ should not trigger unused warnings
    assert_no_warning(
        r#"
PROGRAM Main
VAR
    _unused : INT := 0;
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
"#,
        DiagnosticCode::UnusedVariable,
    );
}

#[test]
fn all_variables_used() {
    assert_no_warning(
        r#"
PROGRAM Main
VAR
    a : INT := 0;
    b : INT := 0;
END_VAR
    a := 1;
    b := a;
END_PROGRAM
"#,
        DiagnosticCode::UnusedVariable,
    );
}

#[test]
fn unused_parameter_warning() {
    assert_has_warnings(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    used : INT;
    unused : INT;
END_VAR
    Foo := used;
END_FUNCTION
"#,
        &[DiagnosticCode::UnusedParameter],
    );
}

#[test]
fn multiple_unused_variables() {
    let n = count_warnings(
        r#"
PROGRAM Main
VAR
    a : INT := 0;
    b : INT := 0;
    c : INT := 0;
END_VAR
    a := 1;
END_PROGRAM
"#,
        DiagnosticCode::UnusedVariable,
    );
    assert_eq!(n, 2, "Expected 2 unused variable warnings, got {n}");
}

// =============================================================================
// Variable never assigned
// =============================================================================

#[test]
fn variable_used_but_never_assigned() {
    assert_has_warnings(
        r#"
PROGRAM Main
VAR
    x : INT;
    y : INT := 0;
END_VAR
    y := x;
END_PROGRAM
"#,
        &[DiagnosticCode::VariableNeverAssigned],
    );
}

#[test]
fn variable_assigned_and_used_no_warning() {
    assert_no_warning(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    y : INT := 0;
END_VAR
    x := 42;
    y := x;
END_PROGRAM
"#,
        DiagnosticCode::VariableNeverAssigned,
    );
}

#[test]
fn input_variable_not_flagged_as_never_assigned() {
    // VAR_INPUT is provided by caller, should not warn
    assert_no_warning(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    a : INT;
END_VAR
    Foo := a;
END_FUNCTION
"#,
        DiagnosticCode::VariableNeverAssigned,
    );
}

// =============================================================================
// Shadowing warnings
// =============================================================================

#[test]
fn no_shadow_warning_without_global() {
    assert_no_warning(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := 1;
END_PROGRAM
"#,
        DiagnosticCode::ShadowedVariable,
    );
}

#[test]
fn shadow_global_warns() {
    let n = count_warnings(
        r#"
VAR_GLOBAL
    counter : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := 1;
END_PROGRAM
"#,
        DiagnosticCode::ShadowedVariable,
    );
    assert_eq!(n, 1);
}
