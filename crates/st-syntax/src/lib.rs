//! AST types and CST-to-AST conversion for IEC 61131-3 Structured Text.
//!
//! Converts tree-sitter concrete syntax trees into typed Rust AST nodes
//! with source span tracking for LSP integration.

pub mod ast;
pub mod lower;
pub mod multi_file;

/// Convenience: parse source text and lower to AST in one step.
pub fn parse(source: &str) -> lower::LowerResult {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&st_grammar::language())
        .expect("Failed to load ST grammar");
    let tree = parser.parse(source, None).expect("Failed to parse");
    lower::lower(&tree, source)
}
