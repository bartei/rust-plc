//! Multi-file source merging.
//!
//! Parses multiple ST source files and merges their ASTs into a single
//! compilation unit. Used to include the standard library.

use crate::ast::*;
use crate::lower::LowerResult;

/// Parse multiple source strings and merge into a single SourceFile.
/// Items from earlier sources appear first (stdlib before user code).
pub fn parse_multi(sources: &[&str]) -> LowerResult {
    let mut all_items = Vec::new();
    let mut all_errors = Vec::new();
    let mut total_range = TextRange::new(0, 0);

    for source in sources {
        let result = crate::parse(source);
        all_items.extend(result.source_file.items);
        all_errors.extend(result.errors);
        if result.source_file.range.end > total_range.end {
            total_range = result.source_file.range;
        }
    }

    LowerResult {
        source_file: SourceFile {
            items: all_items,
            range: total_range,
        },
        errors: all_errors,
    }
}

/// Load all .st files from a directory and return their contents.
pub fn load_stdlib_dir(dir: &std::path::Path) -> Vec<String> {
    let mut sources = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "st")
                    .unwrap_or(false)
            })
            .map(|e| e.path())
            .collect();
        paths.sort(); // deterministic order
        for path in paths {
            if let Ok(content) = std::fs::read_to_string(&path) {
                sources.push(content);
            }
        }
    }
    sources
}

/// The built-in standard library source code (embedded at compile time).
pub fn builtin_stdlib() -> Vec<&'static str> {
    vec![
        include_str!("../../../stdlib/counters.st"),
        include_str!("../../../stdlib/edge_detection.st"),
        include_str!("../../../stdlib/math.st"),
        include_str!("../../../stdlib/timers.st"),
        include_str!("../../../stdlib/conversions.st"),
    ]
}
