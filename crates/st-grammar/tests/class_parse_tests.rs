//! Grammar-level parse tests for IEC 61131-3 OOP extensions (Phase 12).

use st_grammar::kind;

fn parse_ok(source: &str) {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&st_grammar::language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "Parse errors in source:\n{}\nTree: {}",
        source,
        tree.root_node().to_sexp()
    );
}

fn parse_has_node(source: &str, kind_name: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&st_grammar::language()).unwrap();
    let tree = parser.parse(source, None).unwrap();
    fn find_kind(node: tree_sitter::Node, kind: &str) -> bool {
        if node.kind() == kind {
            return true;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if find_kind(child, kind) {
                return true;
            }
        }
        false
    }
    find_kind(tree.root_node(), kind_name)
}

// =============================================================================
// Basic CLASS parsing
// =============================================================================

#[test]
fn parse_empty_class() {
    parse_ok(r#"
CLASS MyClass
END_CLASS
"#);
}

#[test]
fn parse_class_with_vars() {
    parse_ok(r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
END_CLASS
"#);
}

#[test]
fn parse_class_with_method() {
    parse_ok(r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
METHOD Increment
VAR_INPUT
    step : INT;
END_VAR
    count := count + step;
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_class_with_return_type_method() {
    parse_ok(r#"
CLASS Calculator
METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_class_extends() {
    parse_ok(r#"
CLASS Base
VAR
    x : INT;
END_VAR
END_CLASS

CLASS Derived EXTENDS Base
VAR
    y : INT;
END_VAR
END_CLASS
"#);
}

#[test]
fn parse_class_implements() {
    parse_ok(r#"
INTERFACE ICountable
METHOD GetCount : INT
END_METHOD
END_INTERFACE

CLASS Counter IMPLEMENTS ICountable
VAR
    count : INT := 0;
END_VAR
METHOD GetCount : INT
    GetCount := count;
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_class_extends_and_implements() {
    parse_ok(r#"
CLASS Derived EXTENDS Base IMPLEMENTS IFoo, IBar
VAR
    z : INT;
END_VAR
END_CLASS
"#);
}

#[test]
fn parse_abstract_class() {
    parse_ok(r#"
ABSTRACT CLASS Shape
ABSTRACT METHOD Area : REAL
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_final_class() {
    parse_ok(r#"
FINAL CLASS Singleton
VAR
    instance : INT;
END_VAR
END_CLASS
"#);
}

// =============================================================================
// METHOD parsing
// =============================================================================

#[test]
fn parse_method_access_specifiers() {
    parse_ok(r#"
CLASS MyClass
PUBLIC METHOD DoPublic
END_METHOD
PRIVATE METHOD DoPrivate
END_METHOD
PROTECTED METHOD DoProtected
END_METHOD
INTERNAL METHOD DoInternal
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_method_override() {
    parse_ok(r#"
CLASS Derived EXTENDS Base
OVERRIDE METHOD Process
    // overridden
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_method_final() {
    parse_ok(r#"
CLASS Base
FINAL METHOD Locked
END_METHOD
END_CLASS
"#);
}

#[test]
fn parse_method_with_vars_and_body() {
    parse_ok(r#"
CLASS Calculator
METHOD Compute : REAL
VAR_INPUT
    x : REAL;
    y : REAL;
END_VAR
VAR
    temp : REAL;
END_VAR
    temp := x * y;
    Compute := temp + 1.0;
END_METHOD
END_CLASS
"#);
}

// =============================================================================
// INTERFACE parsing
// =============================================================================

#[test]
fn parse_empty_interface() {
    parse_ok(r#"
INTERFACE IEmpty
END_INTERFACE
"#);
}

#[test]
fn parse_interface_with_methods() {
    parse_ok(r#"
INTERFACE ISerializable
METHOD Serialize : INT
VAR_INPUT
    buffer : INT;
END_VAR
END_METHOD
METHOD Deserialize : INT
END_METHOD
END_INTERFACE
"#);
}

#[test]
fn parse_interface_extends() {
    parse_ok(r#"
INTERFACE IBase
METHOD GetName : INT
END_METHOD
END_INTERFACE

INTERFACE IDerived EXTENDS IBase
METHOD GetId : INT
END_METHOD
END_INTERFACE
"#);
}

// =============================================================================
// PROPERTY parsing
// =============================================================================

#[test]
fn parse_property_get_set() {
    parse_ok(r#"
CLASS MyClass
VAR
    _value : INT;
END_VAR
PROPERTY Value : INT
GET
    Value := _value;
END_GET
SET
    _value := Value;
END_SET
END_PROPERTY
END_CLASS
"#);
}

#[test]
fn parse_property_get_only() {
    parse_ok(r#"
CLASS MyClass
VAR
    _count : INT;
END_VAR
PROPERTY Count : INT
GET
    Count := _count;
END_GET
END_PROPERTY
END_CLASS
"#);
}

#[test]
fn parse_property_with_access() {
    parse_ok(r#"
CLASS MyClass
VAR
    _x : REAL;
END_VAR
PUBLIC PROPERTY X : REAL
GET
    X := _x;
END_GET
SET
    _x := X;
END_SET
END_PROPERTY
END_CLASS
"#);
}

// =============================================================================
// THIS / SUPER keywords
// =============================================================================

#[test]
fn parse_this_expression() {
    let source = r#"
CLASS MyClass
VAR
    x : INT;
END_VAR
METHOD SetX
VAR_INPUT
    val : INT;
END_VAR
    x := val;
END_METHOD
END_CLASS
"#;
    parse_ok(source);
}

// =============================================================================
// Node kind verification
// =============================================================================

#[test]
fn class_produces_correct_node_kind() {
    assert!(parse_has_node(
        "CLASS Foo\nEND_CLASS\n",
        kind::CLASS_DECLARATION
    ));
}

#[test]
fn interface_produces_correct_node_kind() {
    assert!(parse_has_node(
        "INTERFACE IFoo\nEND_INTERFACE\n",
        kind::INTERFACE_DECLARATION
    ));
}

#[test]
fn method_produces_correct_node_kind() {
    assert!(parse_has_node(
        "CLASS Foo\nMETHOD Bar\nEND_METHOD\nEND_CLASS\n",
        kind::METHOD_DECLARATION
    ));
}

#[test]
fn property_produces_correct_node_kind() {
    assert!(parse_has_node(
        "CLASS Foo\nPROPERTY X : INT\nGET\nEND_GET\nEND_PROPERTY\nEND_CLASS\n",
        kind::PROPERTY_DECLARATION
    ));
}

// =============================================================================
// Case insensitivity
// =============================================================================

#[test]
fn parse_class_case_insensitive() {
    parse_ok(r#"
class MyClass
var
    x : int;
end_var
method Increment
    x := x + 1;
end_method
end_class
"#);
}

#[test]
fn parse_interface_case_insensitive() {
    parse_ok(r#"
interface IFoo
method Bar : int
end_method
end_interface
"#);
}

// =============================================================================
// Multiple classes and interfaces
// =============================================================================

#[test]
fn parse_multiple_classes() {
    parse_ok(r#"
CLASS A
VAR
    x : INT;
END_VAR
END_CLASS

CLASS B
VAR
    y : INT;
END_VAR
END_CLASS

CLASS C EXTENDS A IMPLEMENTS IFoo
END_CLASS
"#);
}

#[test]
fn parse_class_with_program() {
    parse_ok(r#"
CLASS Counter
VAR
    count : INT := 0;
END_VAR
METHOD Increment
    count := count + 1;
END_METHOD
END_CLASS

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c.Increment();
END_PROGRAM
"#);
}

#[test]
fn parse_class_multiple_methods() {
    parse_ok(r#"
CLASS Math
PUBLIC METHOD Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_METHOD
PRIVATE METHOD Helper : INT
    Helper := 42;
END_METHOD
PROTECTED METHOD Internal
VAR
    temp : INT;
END_VAR
    temp := 0;
END_METHOD
END_CLASS
"#);
}
