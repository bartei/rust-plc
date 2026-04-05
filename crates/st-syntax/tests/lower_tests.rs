use st_syntax::ast::*;
use st_syntax::parse;

fn parse_ok(source: &str) -> SourceFile {
    let result = parse(source);
    assert!(
        result.errors.is_empty(),
        "Unexpected lowering errors: {:?}",
        result.errors
    );
    result.source_file
}

// =============================================================================
// Programs
// =============================================================================

#[test]
fn test_minimal_program() {
    let sf = parse_ok(
        "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
    );
    assert_eq!(sf.items.len(), 1);
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    assert_eq!(p.name.name, "Main");
    assert_eq!(p.var_blocks.len(), 1);
    assert_eq!(p.var_blocks[0].declarations.len(), 1);
    assert_eq!(p.var_blocks[0].declarations[0].names[0].name, "x");
    assert!(matches!(
        p.var_blocks[0].declarations[0].ty,
        DataType::Elementary(ElementaryType::Int)
    ));
    assert_eq!(p.body.len(), 1);
}

#[test]
fn test_program_with_initial_values() {
    let sf = parse_ok(
        r#"
PROGRAM Init
VAR
    counter : INT := 0;
    flag : BOOL := TRUE;
    pi : REAL := 3.14;
END_VAR
    counter := counter + 1;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    assert_eq!(p.var_blocks[0].declarations.len(), 3);
    // counter := 0
    let init = p.var_blocks[0].declarations[0].initial_value.as_ref().unwrap();
    let Expression::Literal(lit) = init else {
        panic!("expected literal");
    };
    assert!(matches!(lit.kind, LiteralKind::Integer(0)));
}

// =============================================================================
// Functions
// =============================================================================

#[test]
fn test_function_with_return_type() {
    let sf = parse_ok(
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
    let TopLevelItem::Function(f) = &sf.items[0] else {
        panic!("expected Function");
    };
    assert_eq!(f.name.name, "Add");
    assert!(matches!(f.return_type, DataType::Elementary(ElementaryType::Int)));
    assert_eq!(f.var_blocks[0].kind, VarKind::VarInput);
    assert_eq!(f.var_blocks[0].declarations.len(), 2);
}

// =============================================================================
// Function blocks
// =============================================================================

#[test]
fn test_function_block() {
    let sf = parse_ok(
        r#"
FUNCTION_BLOCK Counter
VAR_INPUT
    reset : BOOL;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
VAR
    internal : INT := 0;
END_VAR
    IF reset THEN
        internal := 0;
    ELSE
        internal := internal + 1;
    END_IF;
    count := internal;
END_FUNCTION_BLOCK
"#,
    );
    let TopLevelItem::FunctionBlock(fb) = &sf.items[0] else {
        panic!("expected FunctionBlock");
    };
    assert_eq!(fb.name.name, "Counter");
    assert_eq!(fb.var_blocks.len(), 3);
    assert_eq!(fb.var_blocks[0].kind, VarKind::VarInput);
    assert_eq!(fb.var_blocks[1].kind, VarKind::VarOutput);
    assert_eq!(fb.var_blocks[2].kind, VarKind::Var);
    assert_eq!(fb.body.len(), 2); // if + assignment
}

// =============================================================================
// Type declarations
// =============================================================================

#[test]
fn test_struct_type() {
    let sf = parse_ok(
        r#"
TYPE
    Point : STRUCT
        x : REAL := 0.0;
        y : REAL := 0.0;
    END_STRUCT;
END_TYPE
"#,
    );
    let TopLevelItem::TypeDeclaration(td) = &sf.items[0] else {
        panic!("expected TypeDeclaration");
    };
    assert_eq!(td.definitions.len(), 1);
    let TypeDefKind::Struct(s) = &td.definitions[0].ty else {
        panic!("expected struct");
    };
    assert_eq!(s.fields.len(), 2);
    assert_eq!(s.fields[0].name.name, "x");
    assert!(s.fields[0].default.is_some());
}

#[test]
fn test_enum_type() {
    let sf = parse_ok(
        r#"
TYPE
    Color : (Red, Green, Blue);
END_TYPE
"#,
    );
    let TopLevelItem::TypeDeclaration(td) = &sf.items[0] else {
        panic!("expected TypeDeclaration");
    };
    let TypeDefKind::Enum(e) = &td.definitions[0].ty else {
        panic!("expected enum");
    };
    assert_eq!(e.values.len(), 3);
    assert_eq!(e.values[0].name.name, "Red");
    assert_eq!(e.values[2].name.name, "Blue");
}

#[test]
fn test_array_type() {
    let sf = parse_ok(
        r#"
TYPE
    Matrix : ARRAY[1..3, 1..3] OF REAL;
END_TYPE
"#,
    );
    let TopLevelItem::TypeDeclaration(td) = &sf.items[0] else {
        panic!("expected TypeDeclaration");
    };
    let TypeDefKind::Alias(DataType::Array(arr)) = &td.definitions[0].ty else {
        panic!("expected array alias");
    };
    assert_eq!(arr.ranges.len(), 2);
    assert!(matches!(arr.element_type, DataType::Elementary(ElementaryType::Real)));
}

// =============================================================================
// Statements
// =============================================================================

#[test]
fn test_if_elsif_else() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    x : INT;
END_VAR
    IF x > 10 THEN
        x := 10;
    ELSIF x > 5 THEN
        x := 5;
    ELSE
        x := 0;
    END_IF;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    let Statement::If(if_stmt) = &p.body[0] else {
        panic!("expected if");
    };
    assert_eq!(if_stmt.then_body.len(), 1);
    assert_eq!(if_stmt.elsif_clauses.len(), 1);
    assert!(if_stmt.else_body.is_some());
    assert_eq!(if_stmt.else_body.as_ref().unwrap().len(), 1);
}

#[test]
fn test_case_statement() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    mode : INT;
    x : INT;
END_VAR
    CASE mode OF
        1:
            x := 10;
        2, 3:
            x := 20;
    END_CASE;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    let Statement::Case(case) = &p.body[0] else {
        panic!("expected case");
    };
    assert_eq!(case.branches.len(), 2);
    assert_eq!(case.branches[1].selectors.len(), 2);
}

#[test]
fn test_for_loop() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    i : INT;
    sum : INT := 0;
END_VAR
    FOR i := 1 TO 10 BY 2 DO
        sum := sum + i;
    END_FOR;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    let Statement::For(for_stmt) = &p.body[0] else {
        panic!("expected for");
    };
    assert_eq!(for_stmt.variable.name, "i");
    assert!(for_stmt.by.is_some());
    assert_eq!(for_stmt.body.len(), 1);
}

#[test]
fn test_while_loop() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    x : INT := 100;
END_VAR
    WHILE x > 0 DO
        x := x - 1;
    END_WHILE;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    assert!(matches!(&p.body[0], Statement::While(_)));
}

#[test]
fn test_repeat_loop() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    x : INT := 0;
END_VAR
    REPEAT
        x := x + 1;
    UNTIL x >= 100
    END_REPEAT;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    assert!(matches!(&p.body[0], Statement::Repeat(_)));
}

// =============================================================================
// Expressions
// =============================================================================

#[test]
fn test_binary_expressions() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    a : INT;
    b : INT;
    c : INT;
END_VAR
    a := 1 + 2 * 3;
    b := (a - 1) / 2;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    // a := 1 + 2 * 3  → Add(1, Mul(2, 3))
    let Statement::Assignment(asgn) = &p.body[0] else {
        panic!("expected assignment");
    };
    let Expression::Binary(bin) = &asgn.value else {
        panic!("expected binary");
    };
    assert_eq!(bin.op, BinaryOp::Add);
}

#[test]
fn test_function_call_expression() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    x : INT;
END_VAR
    x := MyFunc(a := 1, b := 2);
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    let Statement::Assignment(asgn) = &p.body[0] else {
        panic!("expected assignment");
    };
    let Expression::FunctionCall(fc) = &asgn.value else {
        panic!("expected function call");
    };
    assert_eq!(fc.name.as_str(), "MyFunc");
    assert_eq!(fc.arguments.len(), 2);
    assert!(matches!(&fc.arguments[0], Argument::Named { .. }));
}

#[test]
fn test_fb_call_statement() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    timer : TON;
END_VAR
    timer(IN := TRUE, PT := T#5s);
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    assert!(matches!(&p.body[0], Statement::FunctionCall(_)));
}

// =============================================================================
// Literals
// =============================================================================

#[test]
fn test_integer_literals() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    x : INT;
END_VAR
    x := 42;
    x := 16#FF;
    x := 2#1010;
    x := 8#77;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    let values: Vec<i64> = p
        .body
        .iter()
        .filter_map(|s| {
            let Statement::Assignment(a) = s else { return None };
            let Expression::Literal(l) = &a.value else { return None };
            let LiteralKind::Integer(v) = &l.kind else { return None };
            Some(*v)
        })
        .collect();
    assert_eq!(values, vec![42, 255, 10, 63]);
}

#[test]
fn test_boolean_and_real_literals() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    b : BOOL;
    r : REAL;
END_VAR
    b := TRUE;
    b := FALSE;
    r := 3.14;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    // TRUE
    let Statement::Assignment(a0) = &p.body[0] else { panic!() };
    let Expression::Literal(l0) = &a0.value else { panic!() };
    assert!(matches!(l0.kind, LiteralKind::Bool(true)));
    // FALSE
    let Statement::Assignment(a1) = &p.body[1] else { panic!() };
    let Expression::Literal(l1) = &a1.value else { panic!() };
    assert!(matches!(l1.kind, LiteralKind::Bool(false)));
    // 3.14
    let Statement::Assignment(a2) = &p.body[2] else { panic!() };
    let Expression::Literal(l2) = &a2.value else { panic!() };
    let LiteralKind::Real(v) = l2.kind else { panic!() };
    #[allow(clippy::approx_constant)]
    let expected = 3.14;
    assert!((v - expected).abs() < 1e-10);
}

// =============================================================================
// Variable access (struct fields, array indexing)
// =============================================================================

#[test]
fn test_struct_field_access() {
    let sf = parse_ok(
        r#"
PROGRAM Test
VAR
    pid : PID_Controller;
    out : REAL;
END_VAR
    out := pid.output;
END_PROGRAM
"#,
    );
    let TopLevelItem::Program(p) = &sf.items[0] else {
        panic!("expected Program");
    };
    let Statement::Assignment(a) = &p.body[0] else { panic!() };
    let Expression::Variable(va) = &a.value else {
        panic!("expected variable access");
    };
    assert_eq!(va.parts.len(), 2);
    let AccessPart::Identifier(id0) = &va.parts[0] else { panic!() };
    let AccessPart::Identifier(id1) = &va.parts[1] else { panic!() };
    assert_eq!(id0.name, "pid");
    assert_eq!(id1.name, "output");
}

// =============================================================================
// Source ranges
// =============================================================================

#[test]
fn test_ranges_are_nonzero() {
    let sf = parse_ok(
        "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
    );
    assert!(sf.range.end > sf.range.start);
    let TopLevelItem::Program(p) = &sf.items[0] else { panic!() };
    assert!(p.range.end > p.range.start);
    assert!(p.name.range.end > p.name.range.start);
}

// =============================================================================
// Error recovery
// =============================================================================

#[test]
fn test_partial_ast_on_error() {
    let result = parse(
        "PROGRAM Broken\nVAR\n    x : INT;\nEND_VAR\n    x := ;\nEND_PROGRAM\n",
    );
    // Should still produce a source file with a program
    assert_eq!(result.source_file.items.len(), 1);
    // Errors should be reported
    assert!(!result.errors.is_empty());
}

// =============================================================================
// Multiple POUs in one file
// =============================================================================

#[test]
fn test_multiple_pous() {
    let sf = parse_ok(
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
    result : INT;
END_VAR
    result := Add(a := 1, b := 2);
END_PROGRAM
"#,
    );
    assert_eq!(sf.items.len(), 2);
    assert!(matches!(&sf.items[0], TopLevelItem::Function(_)));
    assert!(matches!(&sf.items[1], TopLevelItem::Program(_)));
}
