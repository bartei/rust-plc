//! Acceptance tests for IEC 61131-3 / CODESYS-compatible string functions.
//!
//! Covers every Tier-5 string intrinsic with happy-path + edge cases:
//! out-of-range indices, position 0, negative arguments, empty strings,
//! parse failures, round-trips. Test bodies compile a small ST program
//! against the embedded stdlib and assert on global variables.

use st_engine::*;
use st_ir::*;

/// Compile + run an ST source for `cycles` cycles, returning the engine
/// so individual tests can assert on globals.
fn run(source: &str, cycles: u64) -> Engine {
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    assert!(parse_result.errors.is_empty(), "parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).expect("compile failed");
    let program_name = module
        .functions
        .iter()
        .find(|f| f.kind == PouKind::Program)
        .expect("no PROGRAM in source")
        .name
        .clone();
    let mut engine = Engine::new(module, program_name, EngineConfig::default());
    for _ in 0..cycles {
        engine.run_one_cycle().unwrap();
    }
    engine
}

/// Convenience: assert a STRING global has the given value.
#[track_caller]
fn assert_str(engine: &Engine, name: &str, expected: &str) {
    match engine.vm().get_global(name) {
        Some(Value::String(s)) => assert_eq!(s, expected, "global {name}"),
        other => panic!("global {name}: expected STRING({expected:?}), got {other:?}"),
    }
}

/// Convenience: assert an INT global has the given value.
#[track_caller]
fn assert_int(engine: &Engine, name: &str, expected: i64) {
    match engine.vm().get_global(name) {
        Some(Value::Int(i)) => assert_eq!(*i, expected, "global {name}"),
        other => panic!("global {name}: expected INT({expected}), got {other:?}"),
    }
}

/// Wrap a body inside a minimal program with VAR_GLOBAL `g` declared as
/// `ty` and `r` as STRING. Lets tests focus on a single expression.
fn one_var_program(ty: &str, body: &str) -> String {
    format!(
        r#"
VAR_GLOBAL
    g : {ty};
END_VAR
PROGRAM Main
VAR
    a : STRING := 'abcdef';
    e : STRING := '';
    h : STRING := 'hello';
    s : STRING := 'rust-plc';
    ws : STRING := '  spaced  ';
END_VAR
    {body}
END_PROGRAM
"#
    )
}

// ============================================================================
// LEN
// ============================================================================

#[test]
fn len_returns_byte_length() {
    let src = one_var_program("INT", "g := LEN(IN := h);");
    assert_int(&run(&src, 1), "g", 5);
}

#[test]
fn len_empty_is_zero() {
    let src = one_var_program("INT", "g := LEN(IN := e);");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn len_literal() {
    let src = one_var_program("INT", "g := LEN(IN := 'twelve chars');");
    assert_int(&run(&src, 1), "g", 12);
}

// ============================================================================
// LEFT
// ============================================================================

#[test]
fn left_basic() {
    let src = one_var_program("STRING", "g := LEFT(STR := a, SIZE := 3);");
    assert_str(&run(&src, 1), "g", "abc");
}

#[test]
fn left_full_length() {
    let src = one_var_program("STRING", "g := LEFT(STR := a, SIZE := 6);");
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn left_zero() {
    let src = one_var_program("STRING", "g := LEFT(STR := a, SIZE := 0);");
    assert_str(&run(&src, 1), "g", "");
}

#[test]
fn left_oversized_is_full_string() {
    let src = one_var_program("STRING", "g := LEFT(STR := a, SIZE := 100);");
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn left_negative_is_empty() {
    let src = one_var_program("STRING", "g := LEFT(STR := a, SIZE := -3);");
    assert_str(&run(&src, 1), "g", "");
}

#[test]
fn left_of_empty_is_empty() {
    let src = one_var_program("STRING", "g := LEFT(STR := e, SIZE := 5);");
    assert_str(&run(&src, 1), "g", "");
}

// ============================================================================
// RIGHT
// ============================================================================

#[test]
fn right_basic() {
    let src = one_var_program("STRING", "g := RIGHT(STR := a, SIZE := 3);");
    assert_str(&run(&src, 1), "g", "def");
}

#[test]
fn right_full_length() {
    let src = one_var_program("STRING", "g := RIGHT(STR := a, SIZE := 6);");
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn right_zero() {
    let src = one_var_program("STRING", "g := RIGHT(STR := a, SIZE := 0);");
    assert_str(&run(&src, 1), "g", "");
}

#[test]
fn right_oversized_is_full_string() {
    let src = one_var_program("STRING", "g := RIGHT(STR := a, SIZE := 100);");
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn right_negative_is_empty() {
    let src = one_var_program("STRING", "g := RIGHT(STR := a, SIZE := -2);");
    assert_str(&run(&src, 1), "g", "");
}

// ============================================================================
// MID — 1-indexed
// ============================================================================

#[test]
fn mid_basic() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 3, POS := 2);");
    assert_str(&run(&src, 1), "g", "bcd");
}

#[test]
fn mid_at_start() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 2, POS := 1);");
    assert_str(&run(&src, 1), "g", "ab");
}

#[test]
fn mid_at_end() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 2, POS := 5);");
    assert_str(&run(&src, 1), "g", "ef");
}

#[test]
fn mid_overrun_clamps() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 100, POS := 4);");
    assert_str(&run(&src, 1), "g", "def");
}

#[test]
fn mid_pos_zero_is_empty() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 3, POS := 0);");
    assert_str(&run(&src, 1), "g", "");
}

#[test]
fn mid_negative_pos_is_empty() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 3, POS := -1);");
    assert_str(&run(&src, 1), "g", "");
}

#[test]
fn mid_zero_len_is_empty() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 0, POS := 2);");
    assert_str(&run(&src, 1), "g", "");
}

#[test]
fn mid_pos_past_end_is_empty() {
    let src = one_var_program("STRING", "g := MID(STR := a, LEN := 3, POS := 100);");
    assert_str(&run(&src, 1), "g", "");
}

// ============================================================================
// CONCAT
// ============================================================================

#[test]
fn concat_basic() {
    let src = one_var_program("STRING", "g := CONCAT(STR1 := 'foo', STR2 := 'bar');");
    assert_str(&run(&src, 1), "g", "foobar");
}

#[test]
fn concat_with_empty_first() {
    let src = one_var_program("STRING", "g := CONCAT(STR1 := e, STR2 := 'x');");
    assert_str(&run(&src, 1), "g", "x");
}

#[test]
fn concat_with_empty_second() {
    let src = one_var_program("STRING", "g := CONCAT(STR1 := 'x', STR2 := e);");
    assert_str(&run(&src, 1), "g", "x");
}

#[test]
fn concat_both_empty() {
    let src = one_var_program("STRING", "g := CONCAT(STR1 := e, STR2 := e);");
    assert_str(&run(&src, 1), "g", "");
}

// ============================================================================
// INSERT
// ============================================================================

#[test]
fn insert_middle() {
    let src = one_var_program(
        "STRING",
        "g := INSERT(STR1 := 'abef', STR2 := 'cd', POS := 2);",
    );
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn insert_at_start_pos_zero() {
    let src = one_var_program(
        "STRING",
        "g := INSERT(STR1 := a, STR2 := 'XYZ', POS := 0);",
    );
    assert_str(&run(&src, 1), "g", "XYZabcdef");
}

#[test]
fn insert_at_end() {
    let src = one_var_program(
        "STRING",
        "g := INSERT(STR1 := a, STR2 := 'XYZ', POS := 6);",
    );
    assert_str(&run(&src, 1), "g", "abcdefXYZ");
}

#[test]
fn insert_past_end_appends() {
    let src = one_var_program(
        "STRING",
        "g := INSERT(STR1 := a, STR2 := 'XYZ', POS := 100);",
    );
    assert_str(&run(&src, 1), "g", "abcdefXYZ");
}

#[test]
fn insert_empty_str2_is_passthrough() {
    let src = one_var_program(
        "STRING",
        "g := INSERT(STR1 := a, STR2 := e, POS := 3);",
    );
    assert_str(&run(&src, 1), "g", "abcdef");
}

// ============================================================================
// DELETE
// ============================================================================

#[test]
fn delete_middle() {
    let src = one_var_program(
        "STRING",
        "g := DELETE(STR := a, LEN := 2, POS := 3);",
    );
    assert_str(&run(&src, 1), "g", "abef");
}

#[test]
fn delete_at_start() {
    let src = one_var_program(
        "STRING",
        "g := DELETE(STR := a, LEN := 2, POS := 1);",
    );
    assert_str(&run(&src, 1), "g", "cdef");
}

#[test]
fn delete_at_end() {
    let src = one_var_program(
        "STRING",
        "g := DELETE(STR := a, LEN := 2, POS := 5);",
    );
    assert_str(&run(&src, 1), "g", "abcd");
}

#[test]
fn delete_pos_zero_no_op() {
    let src = one_var_program(
        "STRING",
        "g := DELETE(STR := a, LEN := 2, POS := 0);",
    );
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn delete_zero_length_no_op() {
    let src = one_var_program(
        "STRING",
        "g := DELETE(STR := a, LEN := 0, POS := 2);",
    );
    assert_str(&run(&src, 1), "g", "abcdef");
}

#[test]
fn delete_overrun_clamps() {
    let src = one_var_program(
        "STRING",
        "g := DELETE(STR := a, LEN := 100, POS := 3);",
    );
    assert_str(&run(&src, 1), "g", "ab");
}

// ============================================================================
// REPLACE
// ============================================================================

#[test]
fn replace_middle() {
    let src = one_var_program(
        "STRING",
        "g := REPLACE(STR1 := a, STR2 := 'XY', LEN := 3, POS := 2);",
    );
    assert_str(&run(&src, 1), "g", "aXYef");
}

#[test]
fn replace_at_start_pos_one() {
    let src = one_var_program(
        "STRING",
        "g := REPLACE(STR1 := a, STR2 := 'XY', LEN := 2, POS := 1);",
    );
    assert_str(&run(&src, 1), "g", "XYcdef");
}

#[test]
fn replace_with_empty_acts_as_delete() {
    let src = one_var_program(
        "STRING",
        "g := REPLACE(STR1 := a, STR2 := e, LEN := 2, POS := 3);",
    );
    assert_str(&run(&src, 1), "g", "abef");
}

#[test]
fn replace_zero_length_acts_as_insert() {
    let src = one_var_program(
        "STRING",
        "g := REPLACE(STR1 := a, STR2 := 'XY', LEN := 0, POS := 3);",
    );
    assert_str(&run(&src, 1), "g", "abXYcdef");
}

#[test]
fn replace_pos_zero_inserts_at_start_then_drops_len() {
    let src = one_var_program(
        "STRING",
        "g := REPLACE(STR1 := a, STR2 := 'XY', LEN := 2, POS := 0);",
    );
    assert_str(&run(&src, 1), "g", "XYcdef");
}

#[test]
fn replace_past_end_appends() {
    let src = one_var_program(
        "STRING",
        "g := REPLACE(STR1 := a, STR2 := 'XY', LEN := 1, POS := 100);",
    );
    assert_str(&run(&src, 1), "g", "abcdefXY");
}

// ============================================================================
// FIND — 1-based, 0 if not found
// ============================================================================

#[test]
fn find_present() {
    let src = one_var_program("INT", "g := FIND(STR1 := s, STR2 := 'plc');");
    assert_int(&run(&src, 1), "g", 6);
}

#[test]
fn find_first_match_only() {
    let src = one_var_program("INT", "g := FIND(STR1 := 'aaaa', STR2 := 'a');");
    assert_int(&run(&src, 1), "g", 1);
}

#[test]
fn find_at_start() {
    let src = one_var_program("INT", "g := FIND(STR1 := 'rust', STR2 := 'r');");
    assert_int(&run(&src, 1), "g", 1);
}

#[test]
fn find_absent_returns_zero() {
    let src = one_var_program("INT", "g := FIND(STR1 := s, STR2 := 'XYZ');");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn find_empty_needle_returns_zero() {
    let src = one_var_program("INT", "g := FIND(STR1 := s, STR2 := e);");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn find_in_empty_haystack_returns_zero() {
    let src = one_var_program("INT", "g := FIND(STR1 := e, STR2 := 'x');");
    assert_int(&run(&src, 1), "g", 0);
}

// ============================================================================
// Case conversion
// ============================================================================

#[test]
fn to_upper_converts_ascii() {
    let src = one_var_program("STRING", "g := TO_UPPER(IN := 'Hello, World!');");
    assert_str(&run(&src, 1), "g", "HELLO, WORLD!");
}

#[test]
fn to_lower_converts_ascii() {
    let src = one_var_program("STRING", "g := TO_LOWER(IN := 'Hello, World!');");
    assert_str(&run(&src, 1), "g", "hello, world!");
}

#[test]
fn upper_case_alias() {
    let src = one_var_program("STRING", "g := UPPER_CASE(IN := 'mixed Case');");
    assert_str(&run(&src, 1), "g", "MIXED CASE");
}

#[test]
fn lower_case_alias() {
    let src = one_var_program("STRING", "g := LOWER_CASE(IN := 'MIXED Case');");
    assert_str(&run(&src, 1), "g", "mixed case");
}

#[test]
fn case_idempotent_on_empty() {
    let src = one_var_program("STRING", "g := TO_UPPER(IN := e);");
    assert_str(&run(&src, 1), "g", "");
}

// ============================================================================
// Trim
// ============================================================================

#[test]
fn trim_strips_both_sides() {
    let src = one_var_program("STRING", "g := TRIM(IN := ws);");
    assert_str(&run(&src, 1), "g", "spaced");
}

#[test]
fn ltrim_strips_left_only() {
    let src = one_var_program("STRING", "g := LTRIM(IN := ws);");
    assert_str(&run(&src, 1), "g", "spaced  ");
}

#[test]
fn rtrim_strips_right_only() {
    let src = one_var_program("STRING", "g := RTRIM(IN := ws);");
    assert_str(&run(&src, 1), "g", "  spaced");
}

#[test]
fn trim_no_whitespace_passthrough() {
    let src = one_var_program("STRING", "g := TRIM(IN := 'hello');");
    assert_str(&run(&src, 1), "g", "hello");
}

#[test]
fn trim_all_whitespace_yields_empty() {
    let src = one_var_program("STRING", "g := TRIM(IN := '   ');");
    assert_str(&run(&src, 1), "g", "");
}

// ============================================================================
// Numeric → STRING
// ============================================================================

#[test]
fn int_to_string_basic() {
    let src = one_var_program("STRING", "g := INT_TO_STRING(IN := 42);");
    assert_str(&run(&src, 1), "g", "42");
}

#[test]
fn int_to_string_negative() {
    let src = one_var_program("STRING", "g := INT_TO_STRING(IN := -7);");
    assert_str(&run(&src, 1), "g", "-7");
}

#[test]
fn int_to_string_zero() {
    let src = one_var_program("STRING", "g := INT_TO_STRING(IN := 0);");
    assert_str(&run(&src, 1), "g", "0");
}

#[test]
fn dint_to_string_large() {
    let src = one_var_program("STRING", "g := DINT_TO_STRING(IN := 1234567);");
    assert_str(&run(&src, 1), "g", "1234567");
}

#[test]
fn real_to_string_fraction() {
    let src = one_var_program("STRING", "g := REAL_TO_STRING(IN := 3.5);");
    assert_str(&run(&src, 1), "g", "3.5");
}

#[test]
fn real_to_string_whole_number_keeps_decimal() {
    // 1.0 must format as "1.0", not "1" — CODESYS prints REAL with a decimal point.
    let src = one_var_program("STRING", "g := REAL_TO_STRING(IN := 1.0);");
    assert_str(&run(&src, 1), "g", "1.0");
}

#[test]
fn bool_to_string_true() {
    let src = one_var_program("STRING", "g := BOOL_TO_STRING(IN := TRUE);");
    assert_str(&run(&src, 1), "g", "TRUE");
}

#[test]
fn bool_to_string_false() {
    let src = one_var_program("STRING", "g := BOOL_TO_STRING(IN := FALSE);");
    assert_str(&run(&src, 1), "g", "FALSE");
}

#[test]
fn to_string_overload_int() {
    let src = one_var_program("STRING", "g := TO_STRING(IN := 99);");
    assert_str(&run(&src, 1), "g", "99");
}

#[test]
fn to_string_overload_bool() {
    let src = one_var_program("STRING", "g := TO_STRING(IN := FALSE);");
    assert_str(&run(&src, 1), "g", "FALSE");
}

#[test]
fn any_to_string_real() {
    let src = one_var_program("STRING", "g := ANY_TO_STRING(IN := 2.25);");
    assert_str(&run(&src, 1), "g", "2.25");
}

// ============================================================================
// STRING → numeric / bool
// ============================================================================

#[test]
fn string_to_int_basic() {
    let src = one_var_program("INT", "g := STRING_TO_INT(IN := '123');");
    assert_int(&run(&src, 1), "g", 123);
}

#[test]
fn string_to_int_negative() {
    let src = one_var_program("INT", "g := STRING_TO_INT(IN := '-99');");
    assert_int(&run(&src, 1), "g", -99);
}

#[test]
fn string_to_int_with_whitespace_trimmed() {
    let src = one_var_program("INT", "g := STRING_TO_INT(IN := '  42  ');");
    assert_int(&run(&src, 1), "g", 42);
}

#[test]
fn string_to_int_garbage_returns_zero() {
    let src = one_var_program("INT", "g := STRING_TO_INT(IN := 'banana');");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn string_to_int_empty_returns_zero() {
    let src = one_var_program("INT", "g := STRING_TO_INT(IN := '');");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn string_to_real_basic() {
    let src = one_var_program("REAL", "g := STRING_TO_REAL(IN := '2.5');");
    let engine = run(&src, 1);
    match engine.vm().get_global("g") {
        Some(Value::Real(r)) => assert!((r - 2.5).abs() < 1e-9, "got {r}"),
        other => panic!("expected REAL, got {other:?}"),
    }
}

#[test]
fn string_to_real_garbage_returns_zero() {
    let src = one_var_program("REAL", "g := STRING_TO_REAL(IN := 'nope');");
    let engine = run(&src, 1);
    assert_eq!(engine.vm().get_global("g"), Some(&Value::Real(0.0)));
}

#[test]
fn string_to_bool_true_word() {
    let src = one_var_program("INT", "g := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := 'TRUE'));");
    assert_int(&run(&src, 1), "g", 1);
}

#[test]
fn string_to_bool_true_lowercase() {
    let src = one_var_program("INT", "g := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := 'true'));");
    assert_int(&run(&src, 1), "g", 1);
}

#[test]
fn string_to_bool_one() {
    let src = one_var_program("INT", "g := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := '1'));");
    assert_int(&run(&src, 1), "g", 1);
}

#[test]
fn string_to_bool_false_word() {
    let src = one_var_program("INT", "g := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := 'FALSE'));");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn string_to_bool_anything_else_false() {
    let src = one_var_program("INT", "g := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := 'maybe'));");
    assert_int(&run(&src, 1), "g", 0);
}

#[test]
fn string_to_bool_empty_false() {
    let src = one_var_program("INT", "g := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := ''));");
    assert_int(&run(&src, 1), "g", 0);
}

// ============================================================================
// Round-trips
// ============================================================================

#[test]
fn roundtrip_int_string_int() {
    let src = r#"
VAR_GLOBAL
    g : INT;
END_VAR
PROGRAM Main
VAR
    n : INT := 12345;
    s : STRING;
END_VAR
    s := INT_TO_STRING(IN := n);
    g := STRING_TO_INT(IN := s);
END_PROGRAM
"#;
    assert_int(&run(src, 1), "g", 12345);
}

#[test]
fn roundtrip_int_negative() {
    let src = r#"
VAR_GLOBAL
    g : INT;
END_VAR
PROGRAM Main
VAR
    n : INT := -32768;
    s : STRING;
END_VAR
    s := INT_TO_STRING(IN := n);
    g := STRING_TO_INT(IN := s);
END_PROGRAM
"#;
    assert_int(&run(src, 1), "g", -32768);
}

#[test]
fn roundtrip_real_through_string() {
    let src = r#"
VAR_GLOBAL
    g : REAL;
END_VAR
PROGRAM Main
VAR
    n : REAL := 6.25;
    s : STRING;
END_VAR
    s := REAL_TO_STRING(IN := n);
    g := STRING_TO_REAL(IN := s);
END_PROGRAM
"#;
    let engine = run(src, 1);
    match engine.vm().get_global("g") {
        Some(Value::Real(r)) => assert!((r - 6.25).abs() < 1e-9),
        other => panic!("expected REAL, got {other:?}"),
    }
}

// ============================================================================
// Composition
// ============================================================================

#[test]
fn nested_concat_left_right() {
    let src = r#"
VAR_GLOBAL
    g : STRING;
END_VAR
PROGRAM Main
VAR
    s : STRING := 'abcdef';
END_VAR
    g := CONCAT(STR1 := LEFT(STR := s, SIZE := 2),
                STR2 := RIGHT(STR := s, SIZE := 2));
END_PROGRAM
"#;
    assert_str(&run(src, 1), "g", "abef");
}

#[test]
fn find_then_mid() {
    // Find the substring 'plc' in 'rust-plc-rocks' and return the trailing slice
    // starting at that position with length 8.
    let src = r#"
VAR_GLOBAL
    g : STRING;
END_VAR
PROGRAM Main
VAR
    s : STRING := 'rust-plc-rocks';
    p : INT;
END_VAR
    p := FIND(STR1 := s, STR2 := 'plc');
    g := MID(STR := s, LEN := 8, POS := p);
END_PROGRAM
"#;
    assert_str(&run(src, 1), "g", "plc-rock");
}

#[test]
fn case_pipeline() {
    let src = r#"
VAR_GLOBAL
    g : STRING;
END_VAR
PROGRAM Main
VAR
    s : STRING := '  Hello World  ';
END_VAR
    g := TO_UPPER(IN := TRIM(IN := s));
END_PROGRAM
"#;
    assert_str(&run(src, 1), "g", "HELLO WORLD");
}
