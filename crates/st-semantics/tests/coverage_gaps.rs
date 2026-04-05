//! Tests targeting uncovered paths in the semantic analyzer.

mod test_helpers;
use st_semantics::diagnostic::DiagnosticCode;
use test_helpers::*;

// =============================================================================
// Time type operations
// =============================================================================

#[test]
fn time_addition() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    t1 : TIME := T#5s;
    t2 : TIME := T#10s;
    t3 : TIME := T#0s;
END_VAR
    t3 := t1 + t2;
END_PROGRAM
"#,
    );
}

#[test]
fn time_subtraction() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    t1 : TIME := T#10s;
    t2 : TIME := T#5s;
    t3 : TIME := T#0s;
END_VAR
    t3 := t1 - t2;
END_PROGRAM
"#,
    );
}

// =============================================================================
// Type display names
// =============================================================================

#[test]
fn type_display_covers_all_variants() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    // Elementary
    assert_eq!(Ty::Elementary(ElementaryType::Bool).display_name(), "BOOL");
    assert_eq!(Ty::Elementary(ElementaryType::Sint).display_name(), "SINT");
    assert_eq!(Ty::Elementary(ElementaryType::Lint).display_name(), "LINT");
    assert_eq!(Ty::Elementary(ElementaryType::Usint).display_name(), "USINT");
    assert_eq!(Ty::Elementary(ElementaryType::Ulint).display_name(), "ULINT");
    assert_eq!(Ty::Elementary(ElementaryType::Lreal).display_name(), "LREAL");
    assert_eq!(Ty::Elementary(ElementaryType::Byte).display_name(), "BYTE");
    assert_eq!(Ty::Elementary(ElementaryType::Word).display_name(), "WORD");
    assert_eq!(Ty::Elementary(ElementaryType::Dword).display_name(), "DWORD");
    assert_eq!(Ty::Elementary(ElementaryType::Lword).display_name(), "LWORD");
    assert_eq!(Ty::Elementary(ElementaryType::Time).display_name(), "TIME");
    assert_eq!(Ty::Elementary(ElementaryType::Ltime).display_name(), "LTIME");
    assert_eq!(Ty::Elementary(ElementaryType::Date).display_name(), "DATE");
    assert_eq!(Ty::Elementary(ElementaryType::Ldate).display_name(), "LDATE");
    assert_eq!(Ty::Elementary(ElementaryType::Tod).display_name(), "TOD");
    assert_eq!(Ty::Elementary(ElementaryType::Ltod).display_name(), "LTOD");
    assert_eq!(Ty::Elementary(ElementaryType::Dt).display_name(), "DT");
    assert_eq!(Ty::Elementary(ElementaryType::Ldt).display_name(), "LDT");

    // Compound types
    assert_eq!(Ty::String { wide: false, max_len: None }.display_name(), "STRING");
    assert_eq!(Ty::String { wide: true, max_len: Some(80) }.display_name(), "WSTRING");
    assert_eq!(Ty::Void.display_name(), "VOID");
    assert_eq!(Ty::Unknown.display_name(), "<unknown>");
    assert_eq!(
        Ty::Array {
            ranges: vec![(1, 10)],
            element_type: Box::new(Ty::Elementary(ElementaryType::Int)),
        }
        .display_name(),
        "ARRAY OF INT"
    );
    assert_eq!(
        Ty::Struct {
            name: "Point".to_string(),
            fields: vec![],
        }
        .display_name(),
        "Point"
    );
    assert_eq!(
        Ty::Enum {
            name: "Color".to_string(),
            variants: vec![],
        }
        .display_name(),
        "Color"
    );
    assert_eq!(
        Ty::FunctionBlock { name: "Timer".to_string() }.display_name(),
        "Timer"
    );
    assert_eq!(
        Ty::Subrange {
            name: "SmallInt".to_string(),
            base: ElementaryType::Int,
            lower: 0,
            upper: 255,
        }
        .display_name(),
        "SmallInt"
    );
}

// =============================================================================
// Type predicates
// =============================================================================

#[test]
fn type_predicates() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    assert!(Ty::Elementary(ElementaryType::Int).is_numeric());
    assert!(Ty::Elementary(ElementaryType::Real).is_numeric());
    assert!(!Ty::Elementary(ElementaryType::Bool).is_numeric());
    assert!(Ty::Elementary(ElementaryType::Int).is_integer());
    assert!(!Ty::Elementary(ElementaryType::Real).is_integer());
    assert!(Ty::Elementary(ElementaryType::Real).is_real());
    assert!(Ty::Elementary(ElementaryType::Lreal).is_real());
    assert!(!Ty::Elementary(ElementaryType::Int).is_real());
    assert!(Ty::Elementary(ElementaryType::Bool).is_bool());
    assert!(!Ty::Elementary(ElementaryType::Int).is_bool());
    assert!(Ty::Elementary(ElementaryType::Byte).is_bit_string());
    assert!(Ty::Elementary(ElementaryType::Word).is_bit_string());
    assert!(Ty::Elementary(ElementaryType::Dword).is_bit_string());
    assert!(Ty::Elementary(ElementaryType::Lword).is_bit_string());
    assert!(!Ty::Elementary(ElementaryType::Int).is_bit_string());
    assert!(Ty::Elementary(ElementaryType::Time).is_time());
    assert!(Ty::Elementary(ElementaryType::Date).is_time());
    assert!(Ty::Elementary(ElementaryType::Tod).is_time());
    assert!(Ty::Elementary(ElementaryType::Ltime).is_time());
    assert!(Ty::Elementary(ElementaryType::Ldate).is_time());
    assert!(Ty::Elementary(ElementaryType::Ltod).is_time());
    assert!(Ty::Elementary(ElementaryType::Dt).is_time());
    assert!(Ty::Elementary(ElementaryType::Ldt).is_time());
    assert!(!Ty::Elementary(ElementaryType::Int).is_time());
}

// =============================================================================
// Type coercion
// =============================================================================

#[test]
fn coercion_same_type() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let int = Ty::Elementary(ElementaryType::Int);
    assert!(can_coerce(&int, &int));
}

#[test]
fn coercion_widening_chain() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let sint = Ty::Elementary(ElementaryType::Sint);
    let int = Ty::Elementary(ElementaryType::Int);
    let dint = Ty::Elementary(ElementaryType::Dint);
    let lint = Ty::Elementary(ElementaryType::Lint);
    let real = Ty::Elementary(ElementaryType::Real);
    let lreal = Ty::Elementary(ElementaryType::Lreal);

    assert!(can_coerce(&sint, &int));
    assert!(can_coerce(&int, &dint));
    assert!(can_coerce(&dint, &lint));
    assert!(can_coerce(&lint, &real));
    assert!(can_coerce(&real, &lreal));

    // Narrowing should fail
    assert!(!can_coerce(&lreal, &int));
    assert!(!can_coerce(&dint, &sint));
}

#[test]
fn coercion_bool_incompatible() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let bool_ty = Ty::Elementary(ElementaryType::Bool);
    let int = Ty::Elementary(ElementaryType::Int);
    assert!(!can_coerce(&bool_ty, &int));
    assert!(!can_coerce(&int, &bool_ty));
}

#[test]
fn common_type_numeric() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let int = Ty::Elementary(ElementaryType::Int);
    let dint = Ty::Elementary(ElementaryType::Dint);
    let result = common_type(&int, &dint);
    assert_eq!(result, Some(Ty::Elementary(ElementaryType::Dint)));

    let real = Ty::Elementary(ElementaryType::Real);
    let result = common_type(&int, &real);
    assert_eq!(result, Some(Ty::Elementary(ElementaryType::Real)));
}

#[test]
fn common_type_incompatible() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let bool_ty = Ty::Elementary(ElementaryType::Bool);
    let int = Ty::Elementary(ElementaryType::Int);
    assert_eq!(common_type(&bool_ty, &int), None);
}

// =============================================================================
// Alias type resolution
// =============================================================================

#[test]
fn alias_type_resolved() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let alias = Ty::Alias {
        name: "MyInt".to_string(),
        target: Box::new(Ty::Elementary(ElementaryType::Int)),
    };
    assert_eq!(alias.display_name(), "MyInt");
    assert_eq!(*alias.resolved(), Ty::Elementary(ElementaryType::Int));
    assert!(alias.resolved().is_integer());
}

// =============================================================================
// Diagnostic construction
// =============================================================================

#[test]
fn diagnostic_constructors() {
    use st_semantics::diagnostic::*;
    use st_syntax::ast::TextRange;

    let e = Diagnostic::error(DiagnosticCode::UndeclaredVariable, "test error", TextRange::new(0, 5));
    assert_eq!(e.severity, Severity::Error);
    assert_eq!(e.code, DiagnosticCode::UndeclaredVariable);

    let w = Diagnostic::warning(DiagnosticCode::UnusedVariable, "test warning", TextRange::new(0, 5));
    assert_eq!(w.severity, Severity::Warning);

    let i = Diagnostic::info(DiagnosticCode::ShadowedVariable, "test info", TextRange::new(0, 5));
    assert_eq!(i.severity, Severity::Info);
}

// =============================================================================
// Scope / symbol table
// =============================================================================

#[test]
fn symbol_table_basic_operations() {
    use st_semantics::scope::*;
    use st_semantics::types::Ty;
    use st_syntax::ast::{TextRange, VarKind, ElementaryType};

    let mut table = SymbolTable::new();
    let global = table.global_scope_id();

    // Define a variable
    table.define(
        global,
        Symbol {
            name: "x".to_string(),
            ty: Ty::Elementary(ElementaryType::Int),
            kind: SymbolKind::Variable(VarKind::Var),
            range: TextRange::new(0, 1),
            used: false,
            assigned: false,
        },
    );

    // Resolve it
    let resolved = table.resolve(global, "x");
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap().1.name, "x");

    // Case insensitive
    let resolved_upper = table.resolve(global, "X");
    assert!(resolved_upper.is_some());

    // Mark used
    table.mark_used(global, "x");
    let sym = table.resolve(global, "x").unwrap().1;
    assert!(sym.used);

    // Mark assigned
    table.mark_assigned(global, "x");
    let sym = table.resolve(global, "x").unwrap().1;
    assert!(sym.assigned);
}

#[test]
fn symbol_table_scope_chain() {
    use st_semantics::scope::*;
    use st_semantics::types::Ty;
    use st_syntax::ast::{TextRange, VarKind, ElementaryType};

    let mut table = SymbolTable::new();
    let global = table.global_scope_id();

    // Global variable
    table.define(
        global,
        Symbol {
            name: "g".to_string(),
            ty: Ty::Elementary(ElementaryType::Int),
            kind: SymbolKind::Variable(VarKind::VarGlobal),
            range: TextRange::new(0, 1),
            used: false,
            assigned: false,
        },
    );

    // Child scope with local variable
    let child = table.create_scope(global, "Main".to_string());
    table.define(
        child,
        Symbol {
            name: "local".to_string(),
            ty: Ty::Elementary(ElementaryType::Bool),
            kind: SymbolKind::Variable(VarKind::Var),
            range: TextRange::new(10, 15),
            used: false,
            assigned: false,
        },
    );

    // Child scope can see global
    assert!(table.resolve(child, "g").is_some());
    // Child scope can see local
    assert!(table.resolve(child, "local").is_some());
    // Global cannot see child's local
    assert!(table.resolve(global, "local").is_none());
}

#[test]
fn symbol_table_type_and_pou_resolution() {
    use st_semantics::scope::*;
    use st_semantics::types::Ty;
    use st_syntax::ast::{TextRange, ElementaryType};

    let mut table = SymbolTable::new();
    let global = table.global_scope_id();

    // Define a type
    table.define(
        global,
        Symbol {
            name: "Point".to_string(),
            ty: Ty::Struct { name: "Point".to_string(), fields: vec![] },
            kind: SymbolKind::Type,
            range: TextRange::new(0, 5),
            used: false,
            assigned: false,
        },
    );

    // Define a function
    table.define(
        global,
        Symbol {
            name: "Add".to_string(),
            ty: Ty::Elementary(ElementaryType::Int),
            kind: SymbolKind::Function {
                return_type: Ty::Elementary(ElementaryType::Int),
                params: vec![],
            },
            range: TextRange::new(10, 13),
            used: false,
            assigned: false,
        },
    );

    assert!(table.resolve_type("Point").is_some());
    assert!(table.resolve_type("Add").is_none()); // not a type

    assert!(table.resolve_pou("Add").is_some());
    assert!(table.resolve_pou("Point").is_none()); // not a POU

    assert!(table.struct_fields("Point").is_some());
    assert!(table.struct_fields("Add").is_none());
}

// =============================================================================
// End-to-end semantic edge cases
// =============================================================================

#[test]
fn string_indexing() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    s : STRING[80];
    ch : BYTE;
END_VAR
    ch := s[1];
END_PROGRAM
"#,
    );
}

#[test]
fn xor_expression() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    a : BOOL := TRUE;
    b : BOOL := FALSE;
    c : BOOL := FALSE;
END_VAR
    c := a XOR b;
END_PROGRAM
"#,
    );
}

#[test]
fn power_expression_type() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : LREAL := 0.0;
END_VAR
    x := 2.0 ** 3.0;
END_PROGRAM
"#,
    );
}

#[test]
fn multiple_vars_same_line() {
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

#[test]
fn nested_function_calls() {
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
    result := Outer(y := Inner(x := 5));
END_PROGRAM
"#,
    );
}

#[test]
fn check_convenience_includes_parse_errors() {
    let result = st_semantics::check("PROGRAM Broken\nVAR\n    x : INT;\nEND_VAR\n    x := ;\nEND_PROGRAM\n");
    assert!(!result.diagnostics.is_empty(), "Should have parse errors");
}

// =============================================================================
// Numeric rank coverage
// =============================================================================

#[test]
fn numeric_rank_all_types() {
    use st_semantics::types::numeric_rank;
    use st_syntax::ast::ElementaryType;

    assert!(numeric_rank(ElementaryType::Sint).is_some());
    assert!(numeric_rank(ElementaryType::Usint).is_some());
    assert!(numeric_rank(ElementaryType::Int).is_some());
    assert!(numeric_rank(ElementaryType::Uint).is_some());
    assert!(numeric_rank(ElementaryType::Dint).is_some());
    assert!(numeric_rank(ElementaryType::Udint).is_some());
    assert!(numeric_rank(ElementaryType::Lint).is_some());
    assert!(numeric_rank(ElementaryType::Ulint).is_some());
    assert!(numeric_rank(ElementaryType::Real).is_some());
    assert!(numeric_rank(ElementaryType::Lreal).is_some());
    assert!(numeric_rank(ElementaryType::Bool).is_none());
    assert!(numeric_rank(ElementaryType::Time).is_none());

    // Verify ordering
    assert!(numeric_rank(ElementaryType::Sint).unwrap() < numeric_rank(ElementaryType::Int).unwrap());
    assert!(numeric_rank(ElementaryType::Int).unwrap() < numeric_rank(ElementaryType::Dint).unwrap());
    assert!(numeric_rank(ElementaryType::Real).unwrap() < numeric_rank(ElementaryType::Lreal).unwrap());
}

// =============================================================================
// Duplicate FB declaration (lines 139-146)
// =============================================================================

#[test]
fn duplicate_function_block_declaration() {
    assert_has_errors(
        r#"
FUNCTION_BLOCK MyFB
VAR_INPUT
    val : INT;
END_VAR
    ;
END_FUNCTION_BLOCK

FUNCTION_BLOCK MyFB
VAR_INPUT
    val : INT;
END_VAR
    ;
END_FUNCTION_BLOCK

PROGRAM Main
    ;
END_PROGRAM
"#,
        &[DiagnosticCode::DuplicateDeclaration],
    );
}

// =============================================================================
// CONSTANT without initial value (lines 300-306)
// =============================================================================

#[test]
fn constant_without_initial_value() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR CONSTANT
    c1 : INT;
END_VAR
    ;
END_PROGRAM
"#,
        &[DiagnosticCode::AssignmentToConstant],
    );
}

// =============================================================================
// Empty statement (line 353)
// =============================================================================

#[test]
fn empty_statement_is_accepted() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    ;
    x := 1;
    ;
END_PROGRAM
"#,
    );
}

// =============================================================================
// check_variable_access_for_write — VarInput branch (lines 383-387, 400, 404)
// =============================================================================

#[test]
fn assign_to_var_input_inside_pou() {
    // Assigning to VAR_INPUT inside the POU body is allowed (local copy).
    assert_no_errors(
        r#"
FUNCTION Foo : INT
VAR_INPUT
    inp : INT;
END_VAR
    inp := 42;
    Foo := inp;
END_FUNCTION

PROGRAM Main
VAR
    r : INT := 0;
END_VAR
    r := Foo(inp := 1);
END_PROGRAM
"#,
    );
}

#[test]
fn assign_to_non_variable_symbol() {
    // Assigning to a function name that's not a return-variable triggers the _ => {} branch
    // at line 400 and also mark_assigned at line 402.
    // A function name IS assignable inside its own body (it sets the return value).
    assert_no_errors(
        r#"
FUNCTION Calc : INT
VAR_INPUT
    x : INT;
END_VAR
    Calc := x + 1;
END_FUNCTION

PROGRAM Main
VAR
    r : INT := 0;
END_VAR
    r := Calc(x := 5);
END_PROGRAM
"#,
    );
}

// =============================================================================
// Case range selector type mismatch (lines 452-461)
// =============================================================================

#[test]
fn case_range_selector_type_mismatch() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    CASE x OF
        1.0..5.0:
            x := 0;
    END_CASE;
END_PROGRAM
"#,
        &[DiagnosticCode::CaseSelectorTypeMismatch],
    );
}

// =============================================================================
// FOR 'by' must be integer (lines 520-524)
// =============================================================================

#[test]
fn for_by_must_be_integer() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    idx : INT := 0;
    step : REAL := 1.0;
END_VAR
    FOR idx := 0 TO 10 BY step DO
        ;
    END_FOR;
END_PROGRAM
"#,
        &[DiagnosticCode::TypeMismatch],
    );
}

// =============================================================================
// Literal types: Date, Tod, Dt (lines 577-580)
// =============================================================================

#[test]
fn date_literal_type() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    d1 : DATE := D#2024-01-01;
END_VAR
    ;
END_PROGRAM
"#,
    );
}

#[test]
fn tod_literal_type() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    t1 : TOD := TOD#12:00:00;
END_VAR
    ;
END_PROGRAM
"#,
    );
}

#[test]
fn dt_literal_type() {
    assert_no_errors(
        r#"
PROGRAM Main
VAR
    mydt : DT := DT#2024-01-01-12:00:00;
END_VAR
    ;
END_PROGRAM
"#,
    );
}

// =============================================================================
// FB instance field access — resolve_pou returns None (lines 651, 653)
// Lines 651: close of `if let Some(sym) = resolve_pou(name)` block
// Line 653: the else branch — return Ty::Unknown when FB type not found
// =============================================================================

#[test]
fn fb_field_access_on_known_fb() {
    // Exercises the FB field-access path (line 627 onward) with a valid FB.
    assert_no_errors(
        r#"
FUNCTION_BLOCK Counter
VAR_INPUT
    increment : INT;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
    count := count + increment;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    c : Counter;
    result : INT := 0;
END_VAR
    c(increment := 1);
    result := c.count;
END_PROGRAM
"#,
    );
}

#[test]
fn fb_field_access_no_such_member() {
    // Exercises the "no member on function block" error path (lines 641-650).
    assert_has_errors(
        r#"
FUNCTION_BLOCK Counter
VAR_INPUT
    increment : INT;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
    count := count + increment;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    c : Counter;
    result : INT := 0;
END_VAR
    c(increment := 1);
    result := c.nonexistent;
END_PROGRAM
"#,
        &[DiagnosticCode::NoSuchField],
    );
}

// =============================================================================
// String indexing with non-integer index (lines 708-715)
// And Unknown type indexed (line 720)
// =============================================================================

#[test]
fn string_index_non_integer() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    s : STRING[80];
    ch : BYTE;
    r : REAL := 1.0;
END_VAR
    ch := s[r];
END_PROGRAM
"#,
        &[DiagnosticCode::ArrayIndexTypeMismatch],
    );
}

// =============================================================================
// Function call on FB type directly (lines 755-757) — calling FB type as function
// FB instance call (lines 762-776) — calling an FB instance variable
// Calling a non-callable symbol (lines 789-794)
// =============================================================================

#[test]
fn call_fb_type_directly() {
    // Calling a FUNCTION_BLOCK name directly (not an instance) exercises lines 755-757.
    assert_no_errors(
        r#"
FUNCTION_BLOCK Adder
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    ;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    inst : Adder;
END_VAR
    Adder(a := 1, b := 2);
END_PROGRAM
"#,
    );
}

#[test]
fn call_fb_instance_variable() {
    // Calling an FB instance variable exercises lines 762-776.
    assert_no_errors(
        r#"
FUNCTION_BLOCK Worker
VAR_INPUT
    val : INT;
END_VAR
    ;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    w : Worker;
END_VAR
    w(val := 42);
END_PROGRAM
"#,
    );
}

#[test]
fn call_non_callable_symbol() {
    // Calling a plain variable like a function exercises lines 789-794.
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x(42);
END_PROGRAM
"#,
        &[DiagnosticCode::NotCallable],
    );
}

// =============================================================================
// Positional arg type mismatch (lines 869-881)
// =============================================================================

#[test]
fn positional_arg_type_mismatch() {
    assert_has_errors(
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
    r : INT := 0;
END_VAR
    r := Add(1, 2.5);
END_PROGRAM
"#,
        &[DiagnosticCode::ParamTypeMismatch],
    );
}

// =============================================================================
// Missing required VAR_IN_OUT parameter (lines 913-920)
// =============================================================================

#[test]
fn missing_required_var_in_out_param() {
    assert_has_errors(
        r#"
FUNCTION Swap : INT
VAR_IN_OUT
    a : INT;
    b : INT;
END_VAR
    Swap := 0;
END_FUNCTION

PROGRAM Main
VAR
    r : INT := 0;
END_VAR
    r := Swap();
END_PROGRAM
"#,
        &[DiagnosticCode::MissingRequiredParam],
    );
}

// =============================================================================
// Binary op: right operand non-numeric (lines 995-999)
// =============================================================================

#[test]
fn binary_right_operand_non_numeric() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    x : INT := 0;
    b : BOOL := FALSE;
END_VAR
    x := 1 + b;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}

// =============================================================================
// Scope: enum_variants method (lines 197, 202-209)
// =============================================================================

#[test]
fn symbol_table_enum_variants() {
    use st_semantics::scope::*;
    use st_semantics::types::Ty;
    use st_syntax::ast::{ElementaryType, TextRange, VarKind};

    let mut table = SymbolTable::new();
    let global = table.global_scope_id();

    // Define an enum type
    table.define(
        global,
        Symbol {
            name: "Color".to_string(),
            ty: Ty::Enum {
                name: "Color".to_string(),
                variants: vec![
                    "Red".to_string(),
                    "Green".to_string(),
                    "Blue".to_string(),
                ],
            },
            kind: SymbolKind::Type,
            range: TextRange::new(0, 5),
            used: false,
            assigned: false,
        },
    );

    // Define a non-enum type
    table.define(
        global,
        Symbol {
            name: "Counter".to_string(),
            ty: Ty::Elementary(ElementaryType::Int),
            kind: SymbolKind::Variable(VarKind::Var),
            range: TextRange::new(10, 17),
            used: false,
            assigned: false,
        },
    );

    // enum_variants returns Some for an enum
    let variants = table.enum_variants("Color");
    assert!(variants.is_some());
    let v = variants.unwrap();
    assert_eq!(v.len(), 3);
    assert_eq!(v[0], "Red");

    // enum_variants returns None for a non-enum
    assert!(table.enum_variants("Counter").is_none());

    // enum_variants returns None for nonexistent type
    assert!(table.enum_variants("DoesNotExist").is_none());

    // struct_fields returns None for an enum (line 197 of scope.rs)
    assert!(table.struct_fields("Color").is_none());
}

// =============================================================================
// Enum to integer coercion (types.rs line 214)
// =============================================================================

#[test]
fn enum_to_integer_coercion() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let enum_ty = Ty::Enum {
        name: "Color".to_string(),
        variants: vec!["Red".to_string(), "Green".to_string()],
    };
    let int_ty = Ty::Elementary(ElementaryType::Int);
    let real_ty = Ty::Elementary(ElementaryType::Real);

    // Enum -> INT is allowed
    assert!(can_coerce(&enum_ty, &int_ty));
    // Enum -> REAL is NOT allowed (Real is not an integer type)
    assert!(!can_coerce(&enum_ty, &real_ty));
}

// =============================================================================
// common_type same type path (types.rs line 224-225 / line 237)
// =============================================================================

#[test]
fn common_type_same_type_returns_clone() {
    use st_semantics::types::*;
    use st_syntax::ast::ElementaryType;

    let int_ty = Ty::Elementary(ElementaryType::Int);
    // Same type should return early at line 224
    assert_eq!(common_type(&int_ty, &int_ty), Some(int_ty.clone()));

    // Two incompatible non-elementary types hit line 237
    let s1 = Ty::String { wide: false, max_len: None };
    let s2 = Ty::Struct { name: "Foo".to_string(), fields: vec![] };
    assert_eq!(common_type(&s1, &s2), None);
}

// =============================================================================
// Additional coverage: left operand non-numeric in binary (line 982-992)
// Already partially covered, but ensure the exact path is hit.
// =============================================================================

#[test]
fn binary_left_operand_non_numeric() {
    assert_has_errors(
        r#"
PROGRAM Main
VAR
    b : BOOL := FALSE;
    x : INT := 0;
END_VAR
    x := b + 1;
END_PROGRAM
"#,
        &[DiagnosticCode::IncompatibleBinaryOp],
    );
}
