//! Semantic type representations and type operations for IEC 61131-3.

use st_syntax::ast::ElementaryType;

// =========================================================================
// Helper functions for ElementaryType (can't add impl in another crate)
// =========================================================================

pub fn elementary_name(e: ElementaryType) -> &'static str {
    match e {
        ElementaryType::Bool => "BOOL",
        ElementaryType::Sint => "SINT",
        ElementaryType::Int => "INT",
        ElementaryType::Dint => "DINT",
        ElementaryType::Lint => "LINT",
        ElementaryType::Usint => "USINT",
        ElementaryType::Uint => "UINT",
        ElementaryType::Udint => "UDINT",
        ElementaryType::Ulint => "ULINT",
        ElementaryType::Real => "REAL",
        ElementaryType::Lreal => "LREAL",
        ElementaryType::Byte => "BYTE",
        ElementaryType::Word => "WORD",
        ElementaryType::Dword => "DWORD",
        ElementaryType::Lword => "LWORD",
        ElementaryType::Time => "TIME",
        ElementaryType::Ltime => "LTIME",
        ElementaryType::Date => "DATE",
        ElementaryType::Ldate => "LDATE",
        ElementaryType::Tod => "TOD",
        ElementaryType::Ltod => "LTOD",
        ElementaryType::Dt => "DT",
        ElementaryType::Ldt => "LDT",
    }
}

pub fn is_integer(e: ElementaryType) -> bool {
    matches!(
        e,
        ElementaryType::Sint
            | ElementaryType::Int
            | ElementaryType::Dint
            | ElementaryType::Lint
            | ElementaryType::Usint
            | ElementaryType::Uint
            | ElementaryType::Udint
            | ElementaryType::Ulint
    )
}

pub fn is_real_type(e: ElementaryType) -> bool {
    matches!(e, ElementaryType::Real | ElementaryType::Lreal)
}

pub fn is_numeric(e: ElementaryType) -> bool {
    is_integer(e) || is_real_type(e)
}

/// Bit-width for numeric ranking. Higher rank = wider type.
pub fn numeric_rank(e: ElementaryType) -> Option<u8> {
    match e {
        ElementaryType::Sint => Some(1),
        ElementaryType::Usint => Some(2),
        ElementaryType::Int => Some(3),
        ElementaryType::Uint => Some(4),
        ElementaryType::Dint => Some(5),
        ElementaryType::Udint => Some(6),
        ElementaryType::Lint => Some(7),
        ElementaryType::Ulint => Some(8),
        ElementaryType::Real => Some(9),
        ElementaryType::Lreal => Some(10),
        _ => None,
    }
}

/// A resolved semantic type. Richer than the AST's DataType because user-defined
/// types have been resolved to their definitions.
#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    /// An elementary/built-in type.
    Elementary(ElementaryType),
    /// ARRAY[ranges] OF element_type. Ranges stored as (lower, upper) constants.
    Array {
        ranges: Vec<(i64, i64)>,
        element_type: Box<Ty>,
    },
    /// STRING or WSTRING with optional max length.
    String { wide: bool, max_len: Option<u32> },
    /// A named struct type.
    Struct { name: String, fields: Vec<FieldDef> },
    /// A named enum type.
    Enum {
        name: String,
        variants: Vec<String>,
    },
    /// A subrange type.
    Subrange {
        name: String,
        base: ElementaryType,
        lower: i64,
        upper: i64,
    },
    /// A function block instance type.
    FunctionBlock { name: String },
    /// A class instance type.
    Class { name: String },
    /// An interface type.
    Interface { name: String },
    /// Type alias (resolved to the underlying type).
    Alias { name: String, target: Box<Ty> },
    /// Void — used for programs (no return value).
    Void,
    /// Unknown — used as a placeholder when resolution fails.
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub ty: Ty,
}

impl Ty {
    /// True if this type is numeric (integer or floating point).
    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Elementary(e) if is_numeric(*e))
    }

    /// True if this type is an integer type.
    pub fn is_integer(&self) -> bool {
        matches!(self, Ty::Elementary(e) if is_integer(*e))
    }

    /// True if this type is a floating-point type.
    pub fn is_real(&self) -> bool {
        matches!(
            self,
            Ty::Elementary(ElementaryType::Real | ElementaryType::Lreal)
        )
    }

    /// True if this is a boolean type.
    pub fn is_bool(&self) -> bool {
        matches!(self, Ty::Elementary(ElementaryType::Bool))
    }

    /// True if this is a bit-string type (BYTE, WORD, DWORD, LWORD).
    pub fn is_bit_string(&self) -> bool {
        matches!(
            self,
            Ty::Elementary(
                ElementaryType::Byte
                    | ElementaryType::Word
                    | ElementaryType::Dword
                    | ElementaryType::Lword
            )
        )
    }

    /// True if this is a time-related type.
    pub fn is_time(&self) -> bool {
        matches!(
            self,
            Ty::Elementary(
                ElementaryType::Time
                    | ElementaryType::Ltime
                    | ElementaryType::Date
                    | ElementaryType::Ldate
                    | ElementaryType::Tod
                    | ElementaryType::Ltod
                    | ElementaryType::Dt
                    | ElementaryType::Ldt
            )
        )
    }

    /// Returns the human-readable name for diagnostics.
    pub fn display_name(&self) -> String {
        match self {
            Ty::Elementary(e) => elementary_name(*e).to_string(),
            Ty::Array { element_type, .. } => format!("ARRAY OF {}", element_type.display_name()),
            Ty::String { wide: false, .. } => "STRING".to_string(),
            Ty::String { wide: true, .. } => "WSTRING".to_string(),
            Ty::Struct { name, .. } => name.clone(),
            Ty::Enum { name, .. } => name.clone(),
            Ty::Subrange { name, .. } => name.clone(),
            Ty::FunctionBlock { name } => name.clone(),
            Ty::Class { name } => name.clone(),
            Ty::Interface { name } => name.clone(),
            Ty::Alias { name, .. } => name.clone(),
            Ty::Void => "VOID".to_string(),
            Ty::Unknown => "<unknown>".to_string(),
        }
    }

    /// Unwrap aliases to get the underlying type.
    pub fn resolved(&self) -> &Ty {
        match self {
            Ty::Alias { target, .. } => target.resolved(),
            other => other,
        }
    }
}

/// Check if an implicit widening coercion from `from` to `to` is allowed.
pub fn can_coerce(from: &Ty, to: &Ty) -> bool {
    let from = from.resolved();
    let to = to.resolved();
    if from == to {
        return true;
    }
    match (from, to) {
        (Ty::Elementary(f), Ty::Elementary(t)) => {
            match (numeric_rank(*f), numeric_rank(*t)) {
                (Some(fr), Some(tr)) => fr <= tr,
                _ => false,
            }
        }
        // Allow enum to integer coercion
        (Ty::Enum { .. }, Ty::Elementary(e)) if is_integer(*e) => true,
        _ => false,
    }
}

/// Find the common type for a binary operation between two types.
/// Returns None if no common type exists.
pub fn common_type(a: &Ty, b: &Ty) -> Option<Ty> {
    let a = a.resolved();
    let b = b.resolved();
    if a == b {
        return Some(a.clone());
    }
    match (a, b) {
        (Ty::Elementary(ea), Ty::Elementary(eb)) => {
            let ra = numeric_rank(*ea)?;
            let rb = numeric_rank(*eb)?;
            if ra >= rb {
                Some(a.clone())
            } else {
                Some(b.clone())
            }
        }
        _ => None,
    }
}
