//! Tests for function and function block call checking.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;
use st_comm_api::native_fb::*;

// =============================================================================
// Function calls — success
// =============================================================================

#[test]
fn call_function_named_args() {
    assert_no_errors(
        r#"
FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := Add(a := 1, b := 2);
END_PROGRAM
"#,
    );
}

#[test]
fn call_function_positional_args() {
    assert_no_errors(
        r#"
FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION

PROGRAM Main
VAR
    result : INT := 0;
END_VAR
    result := Add(1, 2);
END_PROGRAM
"#,
    );
}

#[test]
fn call_fb_instance() {
    assert_no_errors(
        r#"
FUNCTION_BLOCK Counter
VAR_INPUT
    reset : BOOL;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
VAR
    val : INT := 0;
END_VAR
    IF reset THEN
        val := 0;
    ELSE
        val := val + 1;
    END_IF;
    count := val;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    cnt : Counter;
    out : INT := 0;
END_VAR
    cnt(reset := FALSE);
    out := cnt.count;
END_PROGRAM
"#,
    );
}

#[test]
fn call_function_return_type_used() {
    assert_no_errors(
        r#"
FUNCTION Square : REAL
VAR_INPUT
    x : REAL;
END_VAR
    Square := x * x;
END_FUNCTION

PROGRAM Main
VAR
    result : REAL := 0.0;
END_VAR
    result := Square(x := 3.0);
END_PROGRAM
"#,
    );
}

// =============================================================================
// Function calls — failure
// =============================================================================

#[test]
fn call_unknown_function() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := Unknown(a := 1);
END_PROGRAM
"#,
        &[DiagnosticCode::UndeclaredPou],
    );
}

#[test]
fn call_with_unknown_param() {
    assert_has_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    a : INT;
END_VAR
    Foo := a;
END_FUNCTION

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := Foo(nonexistent := 1);
END_PROGRAM
"#,
        &[DiagnosticCode::UnknownParam],
    );
}

#[test]
fn call_with_duplicate_param() {
    assert_has_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    a : INT;
END_VAR
    Foo := a;
END_FUNCTION

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := Foo(a := 1, a := 2);
END_PROGRAM
"#,
        &[DiagnosticCode::DuplicateParam],
    );
}

#[test]
fn call_with_too_many_positional_args() {
    assert_has_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    a : INT;
END_VAR
    Foo := a;
END_FUNCTION

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := Foo(1, 2, 3);
END_PROGRAM
"#,
        &[DiagnosticCode::TooManyPositionalArgs],
    );
}

#[test]
fn call_with_param_type_mismatch() {
    assert_has_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    a : INT;
END_VAR
    Foo := a;
END_FUNCTION

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := Foo(a := 'hello');
END_PROGRAM
"#,
        &[DiagnosticCode::ParamTypeMismatch],
    );
}

#[test]
fn call_non_callable_variable() {
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

#[test]
fn fb_field_access_unknown_member() {
    assert_has_errors(
        r#"
FUNCTION_BLOCK FB1
VAR_OUTPUT
    out : INT;
END_VAR
    out := 42;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    fb : FB1;
    x : INT := 0;
END_VAR
    fb();
    x := fb.nonexistent;
END_PROGRAM
"#,
        &[DiagnosticCode::NoSuchField],
    );
}

#[test]
fn call_with_widening_coercion_param() {
    // Passing SINT to INT parameter should be fine (widening is allowed)
    assert_no_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    a : INT;
END_VAR
    Foo := a;
END_FUNCTION

PROGRAM Main
VAR
    s : SINT;
    x : INT := 0;
END_VAR
    x := Foo(a := s);
END_PROGRAM
"#,
    );
}

#[test]
fn call_function_as_statement() {
    assert_no_errors(
        r#"
FUNCTION_BLOCK Logger
VAR_INPUT
    msg : INT;
END_VAR
VAR
    stored : INT := 0;
END_VAR
    stored := msg;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    log : Logger;
END_VAR
    log(msg := 42);
END_PROGRAM
"#,
    );
}

// =============================================================================
// Native FB tests
// =============================================================================

/// Helper: create a registry with a mock device native FB.
fn mock_device_registry() -> st_comm_api::NativeFbRegistry {
    use st_comm_api::FieldDataType;

    struct MockDevice {
        layout: NativeFbLayout,
    }
    impl NativeFb for MockDevice {
        fn type_name(&self) -> &str {
            &self.layout.type_name
        }
        fn layout(&self) -> &NativeFbLayout {
            &self.layout
        }
        fn execute(&self, _fields: &mut [st_ir::Value]) {}
    }

    let mut reg = st_comm_api::NativeFbRegistry::new();
    reg.register(Box::new(MockDevice {
        layout: NativeFbLayout {
            type_name: "MockIoDevice".to_string(),
            fields: vec![
                NativeFbField {
                    name: "refresh_rate".to_string(),
                    data_type: FieldDataType::Time,
                    var_kind: NativeFbVarKind::VarInput,
                    dimensions: None,
                },
                NativeFbField {
                    name: "connected".to_string(),
                    data_type: FieldDataType::Bool,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
                NativeFbField {
                    name: "DI_0".to_string(),
                    data_type: FieldDataType::Bool,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
                NativeFbField {
                    name: "AI_0".to_string(),
                    data_type: FieldDataType::Int,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
                NativeFbField {
                    name: "DO_0".to_string(),
                    data_type: FieldDataType::Bool,
                    var_kind: NativeFbVarKind::Var,
                    dimensions: None,
                },
            ],
        },
    }));
    reg
}

#[test]
fn native_fb_field_access() {
    let registry = mock_device_registry();
    let result = test_helpers::analyze_with_registry(
        r#"
PROGRAM Main
VAR
    dev : MockIoDevice;
    flag : BOOL;
    val : INT;
END_VAR
    dev(refresh_rate := T#100ms);
    flag := dev.DI_0;
    val := dev.AI_0;
    dev.DO_0 := TRUE;
    IF dev.connected THEN
        flag := TRUE;
    END_IF;
END_PROGRAM
"#,
        &registry,
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == st_semantics::diagnostic::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Expected no errors, got:\n{}",
        errors
            .iter()
            .map(|e| format!("  [{:?}] {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn native_fb_unknown_field() {
    let registry = mock_device_registry();
    let result = test_helpers::analyze_with_registry(
        r#"
PROGRAM Main
VAR
    dev : MockIoDevice;
    x : BOOL;
END_VAR
    dev();
    x := dev.nonexistent_field;
END_PROGRAM
"#,
        &registry,
    );
    let error_codes: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == st_semantics::diagnostic::Severity::Error)
        .map(|d| d.code)
        .collect();
    assert!(
        error_codes.contains(&DiagnosticCode::NoSuchField),
        "Expected NoSuchField error, got: {:?}",
        error_codes
    );
}

#[test]
fn native_fb_without_registry_is_undeclared() {
    // Without a registry, MockIoDevice is not known — should produce an error.
    let result = test_helpers::analyze(
        r#"
PROGRAM Main
VAR
    dev : MockIoDevice;
END_VAR
    dev();
END_PROGRAM
"#,
    );
    let has_error = result
        .diagnostics
        .iter()
        .any(|d| d.severity == st_semantics::diagnostic::Severity::Error);
    assert!(has_error, "Expected undeclared type error without registry");
}
