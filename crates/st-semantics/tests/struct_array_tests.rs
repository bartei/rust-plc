//! Tests for struct field access and array indexing.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// Struct field access — success
// =============================================================================

#[test]
fn struct_field_access() {
    assert_no_errors(
        r#"
TYPE
    Point : STRUCT
        x : REAL := 0.0;
        y : REAL := 0.0;
    END_STRUCT;
END_TYPE

PROGRAM Main
VAR
    p : Point;
    val : REAL := 0.0;
END_VAR
    p.x := 1.0;
    p.y := 2.0;
    val := p.x + p.y;
END_PROGRAM
"#,
    );
}

#[test]
fn nested_struct_field_access() {
    assert_no_errors(
        r#"
TYPE
    Inner : STRUCT
        value : INT := 0;
    END_STRUCT;
    Outer : STRUCT
        inner : Inner;
        name : INT := 0;
    END_STRUCT;
END_TYPE

PROGRAM Main
VAR
    obj : Outer;
    x : INT := 0;
END_VAR
    obj.inner.value := 42;
    x := obj.inner.value;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Struct field access — failure
// =============================================================================

#[test]
fn struct_no_such_field() {
    assert_has_errors(
        r#"
TYPE
    Point : STRUCT
        x : REAL := 0.0;
        y : REAL := 0.0;
    END_STRUCT;
END_TYPE

PROGRAM Main
VAR
    p : Point;
    val : REAL := 0.0;
END_VAR
    val := p.z;
END_PROGRAM
"#,
        &[DiagnosticCode::NoSuchField],
    );
}

#[test]
fn field_access_on_non_struct() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    y : INT := 0;
END_VAR
    y := x.field;
END_PROGRAM
"#,
        &[DiagnosticCode::FieldAccessOnNonStruct],
    );
}

// =============================================================================
// Array indexing — success
// =============================================================================

#[test]
fn array_single_dimension() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    arr : ARRAY[1..10] OF INT;
    i : INT := 1;
    val : INT := 0;
END_VAR
    arr[i] := 42;
    val := arr[1];
END_PROGRAM
"#,
    );
}

#[test]
fn array_multi_dimension() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    matrix : ARRAY[1..3, 1..3] OF REAL;
    val : REAL := 0.0;
END_VAR
    matrix[1, 2] := 3.14;
    val := matrix[2, 3];
END_PROGRAM
"#,
    );
}

#[test]
fn array_of_struct() {
    assert_no_errors(
        r#"
TYPE
    Item : STRUCT
        value : INT := 0;
    END_STRUCT;
END_TYPE

PROGRAM Main
VAR
    items : ARRAY[1..5] OF Item;
    x : INT := 0;
END_VAR
    items[1].value := 42;
    x := items[2].value;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Array indexing — failure
// =============================================================================

#[test]
fn array_index_not_integer() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    arr : ARRAY[1..10] OF INT;
    val : INT := 0;
END_VAR
    val := arr[3.14];
END_PROGRAM
"#,
        &[DiagnosticCode::ArrayIndexTypeMismatch],
    );
}

#[test]
fn array_wrong_dimension_count() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    arr : ARRAY[1..10] OF INT;
    val : INT := 0;
END_VAR
    val := arr[1, 2];
END_PROGRAM
"#,
        &[DiagnosticCode::ArrayDimensionMismatch],
    );
}

#[test]
fn index_on_non_array() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    y : INT := 0;
END_VAR
    y := x[1];
END_PROGRAM
"#,
        &[DiagnosticCode::IndexOnNonArray],
    );
}

#[test]
fn array_bool_index_is_error() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    arr : ARRAY[1..10] OF INT;
    val : INT := 0;
END_VAR
    val := arr[TRUE];
END_PROGRAM
"#,
        &[DiagnosticCode::ArrayIndexTypeMismatch],
    );
}
