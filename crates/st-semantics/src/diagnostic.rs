//! Diagnostic messages produced by semantic analysis.

use st_syntax::ast::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub range: TextRange,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, message: impl Into<String>, range: TextRange) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            range,
        }
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, range: TextRange) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            range,
        }
    }

    pub fn info(code: DiagnosticCode, message: impl Into<String>, range: TextRange) -> Self {
        Self {
            severity: Severity::Info,
            code,
            message: message.into(),
            range,
        }
    }
}

/// Stable diagnostic codes for each error/warning kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    // Scope / name resolution
    UndeclaredVariable,
    UndeclaredPou,
    UndeclaredType,
    DuplicateDeclaration,

    // Type checking
    TypeMismatch,
    TypeMismatchAssignment,
    TypeMismatchCondition,
    TypeMismatchReturn,
    TypeMismatchCaseExpr,
    InvalidOperandType,
    IncompatibleBinaryOp,
    IncompatibleUnaryOp,

    // Array
    ArrayIndexTypeMismatch,
    ArrayDimensionMismatch,
    IndexOnNonArray,

    // Struct
    NoSuchField,
    FieldAccessOnNonStruct,

    // Enum
    InvalidEnumVariant,

    // Function / FB calls
    NotCallable,
    MissingRequiredParam,
    UnknownParam,
    DuplicateParam,
    TooManyPositionalArgs,
    ParamTypeMismatch,

    // Control flow
    ExitOutsideLoop,
    ForVariableNotInteger,
    CaseSelectorTypeMismatch,

    // Constant
    AssignmentToConstant,
    AssignmentToInput,

    // Warnings
    UnusedVariable,
    UnusedParameter,
    VariableNeverAssigned,
    ShadowedVariable,
    DeadCode,
}
