//! Shared test helpers for semantic analysis tests.
#![allow(dead_code)]

use st_semantics::analyze::AnalysisResult;
use st_semantics::diagnostic::{DiagnosticCode, Severity};

/// Parse and analyze source, returning the analysis result.
pub fn analyze(source: &str) -> AnalysisResult {
    let parse_result = st_syntax::parse(source);
    assert!(
        parse_result.errors.is_empty(),
        "Unexpected parse errors: {:?}",
        parse_result.errors
    );
    st_semantics::analyze::analyze(&parse_result.source_file)
}

/// Assert that the analysis produces zero errors (warnings are OK).
pub fn assert_no_errors(source: &str) {
    let result = analyze(source);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
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

/// Assert that the analysis produces zero diagnostics (no errors, no warnings).
pub fn assert_clean(source: &str) {
    let result = analyze(source);
    assert!(
        result.diagnostics.is_empty(),
        "Expected no diagnostics, got:\n{}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("  [{:?}:{:?}] {}", d.severity, d.code, d.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Assert that the analysis produces exactly the given error codes.
pub fn assert_errors(source: &str, expected_codes: &[DiagnosticCode]) {
    let result = analyze(source);
    let actual_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    let actual_codes: Vec<_> = actual_errors.iter().map(|e| e.code).collect();
    assert_eq!(
        actual_codes, expected_codes,
        "Error code mismatch.\nExpected: {:?}\nActual:   {:?}\nMessages:\n{}",
        expected_codes,
        actual_codes,
        actual_errors
            .iter()
            .map(|e| format!("  [{:?}] {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Assert that the analysis produces at least the given error codes (in any order).
pub fn assert_has_errors(source: &str, expected_codes: &[DiagnosticCode]) {
    let result = analyze(source);
    let actual_codes: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|e| e.code)
        .collect();

    for code in expected_codes {
        assert!(
            actual_codes.contains(code),
            "Expected error {code:?} not found.\nActual errors: {actual_codes:?}"
        );
    }
}

/// Assert that the analysis produces at least the given warning codes.
pub fn assert_has_warnings(source: &str, expected_codes: &[DiagnosticCode]) {
    let result = analyze(source);
    let actual_codes: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|e| e.code)
        .collect();

    for code in expected_codes {
        assert!(
            actual_codes.contains(code),
            "Expected warning {:?} not found.\nActual warnings: {:?}\nAll diagnostics:\n{}",
            code,
            actual_codes,
            result
                .diagnostics
                .iter()
                .map(|d| format!("  [{:?}:{:?}] {}", d.severity, d.code, d.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// Assert that no warnings of the given code are produced.
pub fn assert_no_warning(source: &str, code: DiagnosticCode) {
    let result = analyze(source);
    let found: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.code == code)
        .collect();
    assert!(
        found.is_empty(),
        "Unexpected warning {:?}: {}",
        code,
        found.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join(", ")
    );
}

/// Count errors of a specific code.
pub fn count_errors(source: &str, code: DiagnosticCode) -> usize {
    let result = analyze(source);
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == code)
        .count()
}

/// Count warnings of a specific code.
pub fn count_warnings(source: &str, code: DiagnosticCode) -> usize {
    let result = analyze(source);
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.code == code)
        .count()
}
