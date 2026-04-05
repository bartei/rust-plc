//! Tree-sitter grammar for IEC 61131-3 Structured Text.
//!
//! This crate provides the tree-sitter parser generated from the ST grammar
//! definition, along with Rust bindings for incremental parsing.

use tree_sitter::Language;

unsafe extern "C" {
    safe fn tree_sitter_structured_text() -> *const tree_sitter::ffi::TSLanguage;
}

/// Returns the tree-sitter [`Language`] for IEC 61131-3 Structured Text.
pub fn language() -> Language {
    unsafe { Language::from_raw(tree_sitter_structured_text()) }
}

/// Node kind constants matching the grammar's named node types.
pub mod kind {
    pub const SOURCE_FILE: &str = "source_file";
    pub const PROGRAM_DECLARATION: &str = "program_declaration";
    pub const FUNCTION_DECLARATION: &str = "function_declaration";
    pub const FUNCTION_BLOCK_DECLARATION: &str = "function_block_declaration";
    pub const TYPE_DECLARATION: &str = "type_declaration";
    pub const TYPE_DEFINITION: &str = "type_definition";
    pub const STRUCT_TYPE: &str = "struct_type";
    pub const STRUCT_FIELD: &str = "struct_field";
    pub const ENUM_TYPE: &str = "enum_type";
    pub const ENUM_VALUE: &str = "enum_value";
    pub const SUBRANGE_TYPE: &str = "subrange_type";
    pub const ARRAY_TYPE: &str = "array_type";
    pub const ARRAY_RANGE: &str = "array_range";
    pub const VAR_BLOCK: &str = "var_block";
    pub const VAR_KEYWORD: &str = "var_keyword";
    pub const VAR_QUALIFIER: &str = "var_qualifier";
    pub const VARIABLE_DECLARATION: &str = "variable_declaration";
    pub const GLOBAL_VAR_DECLARATION: &str = "global_var_declaration";
    pub const STRING_TYPE: &str = "string_type";
    pub const QUALIFIED_NAME: &str = "qualified_name";
    pub const STATEMENT_LIST: &str = "statement_list";
    pub const ASSIGNMENT_STATEMENT: &str = "assignment_statement";
    pub const FUNCTION_CALL_STATEMENT: &str = "function_call_statement";
    pub const IF_STATEMENT: &str = "if_statement";
    pub const ELSIF_CLAUSE: &str = "elsif_clause";
    pub const ELSE_CLAUSE: &str = "else_clause";
    pub const CASE_STATEMENT: &str = "case_statement";
    pub const CASE_BRANCH: &str = "case_branch";
    pub const CASE_SELECTOR: &str = "case_selector";
    pub const FOR_STATEMENT: &str = "for_statement";
    pub const WHILE_STATEMENT: &str = "while_statement";
    pub const REPEAT_STATEMENT: &str = "repeat_statement";
    pub const RETURN_STATEMENT: &str = "return_statement";
    pub const EXIT_STATEMENT: &str = "exit_statement";
    pub const EMPTY_STATEMENT: &str = "empty_statement";
    pub const OR_EXPRESSION: &str = "or_expression";
    pub const AND_EXPRESSION: &str = "and_expression";
    pub const COMPARISON_EXPRESSION: &str = "comparison_expression";
    pub const ADDITIVE_EXPRESSION: &str = "additive_expression";
    pub const MULTIPLICATIVE_EXPRESSION: &str = "multiplicative_expression";
    pub const POWER_EXPRESSION: &str = "power_expression";
    pub const UNARY_EXPRESSION: &str = "unary_expression";
    pub const PARENTHESIZED_EXPRESSION: &str = "parenthesized_expression";
    pub const VARIABLE_ACCESS: &str = "variable_access";
    pub const FUNCTION_CALL: &str = "function_call";
    pub const ARGUMENT_LIST: &str = "argument_list";
    pub const NAMED_ARGUMENT: &str = "named_argument";
    pub const OUTPUT_ASSIGNMENT: &str = "output_assignment";
    pub const INTEGER_LITERAL: &str = "integer_literal";
    pub const REAL_LITERAL: &str = "real_literal";
    pub const STRING_LITERAL: &str = "string_literal";
    pub const BOOLEAN_LITERAL: &str = "boolean_literal";
    pub const TIME_LITERAL: &str = "time_literal";
    pub const DATE_LITERAL: &str = "date_literal";
    pub const TOD_LITERAL: &str = "tod_literal";
    pub const DT_LITERAL: &str = "dt_literal";
    pub const TYPED_LITERAL: &str = "typed_literal";
    pub const LINE_COMMENT: &str = "line_comment";
    pub const BLOCK_COMMENT: &str = "block_comment";
    pub const IDENTIFIER: &str = "identifier";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language())
            .expect("Failed to load Structured Text grammar");
    }

    #[test]
    fn test_parse_minimal_program() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();

        assert_eq!(root.kind(), kind::SOURCE_FILE);
        assert!(!root.has_error());

        let program = root.child(0).unwrap();
        assert_eq!(program.kind(), kind::PROGRAM_DECLARATION);
        assert_eq!(
            program.child_by_field_name("name").unwrap().utf8_text(source.as_bytes()).unwrap(),
            "Main"
        );
    }

    #[test]
    fn test_parse_function_block() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
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
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors in function block");
    }

    #[test]
    fn test_parse_function_with_return_type() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors in function");
    }

    #[test]
    fn test_parse_type_declarations() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
TYPE
    Color : (Red, Green, Blue);
    Point : STRUCT
        x : REAL := 0.0;
        y : REAL := 0.0;
    END_STRUCT;
    Matrix : ARRAY[1..3, 1..3] OF REAL;
END_TYPE
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors in type declarations");
    }

    #[test]
    fn test_parse_control_flow() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
PROGRAM ControlFlow
VAR
    i : INT;
    x : INT := 0;
    mode : INT := 1;
END_VAR
    FOR i := 1 TO 10 BY 2 DO
        x := x + i;
    END_FOR;

    WHILE x > 0 DO
        x := x - 1;
    END_WHILE;

    REPEAT
        x := x + 1;
    UNTIL x >= 100
    END_REPEAT;

    CASE mode OF
        1:
            x := 10;
        2, 3:
            x := 20;
    END_CASE;
END_PROGRAM
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors in control flow");
    }

    #[test]
    fn test_parse_expressions() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
PROGRAM Expr
VAR
    a : REAL;
    b : REAL;
    c : REAL;
    flag : BOOL;
END_VAR
    a := 1.0 + 2.0 * 3.0;
    b := (a - 1.0) / 2.0;
    c := a ** 2.0;
    flag := a > b AND b < c OR NOT flag;
    a := -b;
END_PROGRAM
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors in expressions");
    }

    #[test]
    fn test_parse_literals() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
PROGRAM Literals
VAR
    i : INT;
    r : REAL;
    b : BOOL;
    t : TIME;
END_VAR
    i := 42;
    i := 16#FF;
    i := 2#1010;
    i := 8#77;
    r := 3.14;
    r := 1.0e10;
    b := TRUE;
    b := FALSE;
    t := T#5s;
END_PROGRAM
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors in literals");
    }

    #[test]
    fn test_parse_comments() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source = r#"
PROGRAM Comments
VAR
    x : INT; // line comment
END_VAR
    (* block comment *)
    x := 1; /* C-style block comment */
    // another line comment
END_PROGRAM
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "Parse errors with comments");
    }

    #[test]
    fn test_error_recovery() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        // Broken syntax: missing expression after :=
        let source = "PROGRAM Broken\nVAR\n    x : INT;\nEND_VAR\n    x := ;\nEND_PROGRAM\n";
        let tree = parser.parse(source, None).unwrap();

        // Should still produce a tree (error recovery)
        assert!(tree.root_node().has_error(), "Expected parse errors");
        // The root should still be a source_file
        assert_eq!(tree.root_node().kind(), kind::SOURCE_FILE);
    }

    #[test]
    fn test_incremental_parse() {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language()).unwrap();

        let source_v1 = "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
        let tree_v1 = parser.parse(source_v1, None).unwrap();
        assert!(!tree_v1.root_node().has_error());

        // Edit: change "x := 1" to "x := 42"
        let source_v2 = "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 42;\nEND_PROGRAM\n";
        let mut old_tree = tree_v1;
        old_tree.edit(&tree_sitter::InputEdit {
            start_byte: 44,
            old_end_byte: 45,
            new_end_byte: 46,
            start_position: tree_sitter::Point { row: 4, column: 9 },
            old_end_position: tree_sitter::Point { row: 4, column: 10 },
            new_end_position: tree_sitter::Point { row: 4, column: 11 },
        });

        let tree_v2 = parser.parse(source_v2, Some(&old_tree)).unwrap();
        assert!(!tree_v2.root_node().has_error());
    }
}
