//! Semantic analysis for IEC 61131-3 Structured Text.
//!
//! Provides symbol table construction, scope resolution, type checking,
//! and diagnostic collection.

pub mod analyze;
pub mod diagnostic;
pub mod scope;
pub mod types;

/// Convenience: parse source text, lower to AST, and analyze in one step.
pub fn check(source: &str) -> analyze::AnalysisResult {
    let parse_result = st_syntax::parse(source);
    let mut result = analyze::analyze(&parse_result.source_file);
    // Prepend parse/lowering errors as diagnostics
    for err in parse_result.errors {
        result.diagnostics.insert(
            0,
            diagnostic::Diagnostic::error(
                diagnostic::DiagnosticCode::UndeclaredVariable, // reuse for parse errors
                err.message,
                err.range,
            ),
        );
    }
    result
}
