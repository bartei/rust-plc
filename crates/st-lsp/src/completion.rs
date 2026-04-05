//! Code completion for Structured Text.

use crate::document::Document;
use st_semantics::scope::{self, ScopeId};
use st_semantics::types::Ty;
use tower_lsp::lsp_types::*;

/// ST keywords for contextual completion.
const KEYWORDS: &[(&str, &str)] = &[
    ("PROGRAM", "PROGRAM ${1:Name}\nVAR\n    $0\nEND_VAR\n\nEND_PROGRAM"),
    ("FUNCTION", "FUNCTION ${1:Name} : ${2:INT}\nVAR_INPUT\n    $0\nEND_VAR\n\nEND_FUNCTION"),
    ("FUNCTION_BLOCK", "FUNCTION_BLOCK ${1:Name}\nVAR_INPUT\n    $0\nEND_VAR\nVAR_OUTPUT\n\nEND_VAR\n\nEND_FUNCTION_BLOCK"),
    ("IF", "IF ${1:condition} THEN\n    $0\nEND_IF;"),
    ("IF_ELSE", "IF ${1:condition} THEN\n    $2\nELSE\n    $0\nEND_IF;"),
    ("FOR", "FOR ${1:i} := ${2:1} TO ${3:10} DO\n    $0\nEND_FOR;"),
    ("WHILE", "WHILE ${1:condition} DO\n    $0\nEND_WHILE;"),
    ("REPEAT", "REPEAT\n    $0\nUNTIL ${1:condition}\nEND_REPEAT;"),
    ("CASE", "CASE ${1:expression} OF\n    ${2:1}:\n        $0\nEND_CASE;"),
    ("VAR", "VAR\n    $0\nEND_VAR"),
    ("VAR_INPUT", "VAR_INPUT\n    $0\nEND_VAR"),
    ("VAR_OUTPUT", "VAR_OUTPUT\n    $0\nEND_VAR"),
    ("TYPE", "TYPE\n    ${1:Name} : $0;\nEND_TYPE"),
    ("STRUCT", "STRUCT\n    $0\nEND_STRUCT"),
    ("ARRAY", "ARRAY[${1:1}..${2:10}] OF ${3:INT}"),
    ("RETURN", "RETURN;"),
    ("EXIT", "EXIT;"),
    ("TRUE", "TRUE"),
    ("FALSE", "FALSE"),
];

/// Elementary type names for type-position completion.
const ELEMENTARY_TYPES: &[&str] = &[
    "BOOL", "SINT", "INT", "DINT", "LINT",
    "USINT", "UINT", "UDINT", "ULINT",
    "REAL", "LREAL",
    "BYTE", "WORD", "DWORD", "LWORD",
    "TIME", "LTIME", "DATE", "LDATE",
    "TOD", "LTOD", "DT", "LDT",
    "STRING", "WSTRING",
];

/// Produce completion items for a given position in the document.
pub fn completions(
    doc: &Document,
    position: Position,
    trigger_char: Option<&str>,
) -> Vec<CompletionItem> {
    let offset = doc.position_to_offset(position);

    // If triggered by '.', provide struct field / FB member completions
    if trigger_char == Some(".") {
        return dot_completions(doc, offset);
    }

    // Get the partial word being typed
    let prefix = get_word_prefix(&doc.source, offset);
    let scope_id = find_scope_for_offset(doc, offset);

    let mut items = Vec::new();

    // Variables in scope
    add_scope_variables(doc, scope_id, &prefix, &mut items);

    // POUs (functions, FBs) from global scope
    add_pous(doc, &prefix, &mut items);

    // User-defined types from global scope
    add_types(doc, &prefix, &mut items);

    // Keywords and snippets
    add_keywords(&prefix, &mut items);

    // Elementary types
    add_elementary_types(&prefix, &mut items);

    items
}

fn dot_completions(doc: &Document, offset: usize) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // Find the identifier before the dot
    let src = doc.source.as_bytes();
    if offset == 0 || (offset > 0 && src[offset - 1] != b'.') {
        return items;
    }

    let dot_pos = offset - 1;
    let end = dot_pos;
    let mut start = end;
    while start > 0 && (src[start - 1].is_ascii_alphanumeric() || src[start - 1] == b'_') {
        start -= 1;
    }

    if start == end {
        return items;
    }

    let var_name = std::str::from_utf8(&src[start..end]).unwrap_or("");
    let scope_id = find_scope_for_offset(doc, dot_pos);

    if let Some((_sid, sym)) = doc.analysis.symbols.resolve(scope_id, var_name) {
        match sym.ty.resolved() {
            Ty::Struct { fields, .. } => {
                for field in fields {
                    items.push(CompletionItem {
                        label: field.name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(field.ty.display_name()),
                        ..Default::default()
                    });
                }
            }
            Ty::FunctionBlock { name } => {
                if let Some(fb_sym) = doc.analysis.symbols.resolve_pou(name) {
                    if let scope::SymbolKind::FunctionBlock { params, outputs } = &fb_sym.kind {
                        for p in outputs {
                            items.push(CompletionItem {
                                label: p.name.clone(),
                                kind: Some(CompletionItemKind::PROPERTY),
                                detail: Some(format!("VAR_OUTPUT : {}", p.ty.display_name())),
                                ..Default::default()
                            });
                        }
                        for p in params {
                            items.push(CompletionItem {
                                label: p.name.clone(),
                                kind: Some(CompletionItemKind::PROPERTY),
                                detail: Some(format!("{:?} : {}", p.var_kind, p.ty.display_name())),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    items
}

fn add_scope_variables(
    doc: &Document,
    scope_id: ScopeId,
    prefix: &str,
    items: &mut Vec<CompletionItem>,
) {
    // Walk the scope chain
    let mut current = Some(scope_id);
    while let Some(sid) = current {
        let scope = doc.analysis.symbols.scope(sid);
        for sym in scope.symbols() {
            if matches!(sym.kind, scope::SymbolKind::Variable(_))
                && matches_prefix(&sym.name, prefix)
            {
                let vk = match &sym.kind {
                    scope::SymbolKind::Variable(vk) => format!("{vk:?}"),
                    _ => String::new(),
                };
                items.push(CompletionItem {
                    label: sym.name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some(format!("{} : {}", vk, sym.ty.display_name())),
                    ..Default::default()
                });
            }
        }
        current = scope.parent;
    }
}

fn add_pous(doc: &Document, prefix: &str, items: &mut Vec<CompletionItem>) {
    let global = doc.analysis.symbols.scope(doc.analysis.symbols.global_scope_id());
    for sym in global.symbols() {
        match &sym.kind {
            scope::SymbolKind::Function { return_type, params } => {
                if matches_prefix(&sym.name, prefix) {
                    let param_list = params
                        .iter()
                        .map(|p| format!("{} : {}", p.name, p.ty.display_name()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let snippet = format!(
                        "{}({})",
                        sym.name,
                        params
                            .iter()
                            .enumerate()
                            .map(|(i, p)| format!("{} := ${{{}:{}}}", p.name, i + 1, p.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    items.push(CompletionItem {
                        label: sym.name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(format!(
                            "FUNCTION({}) : {}",
                            param_list,
                            return_type.display_name()
                        )),
                        insert_text: Some(snippet),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    });
                }
            }
            scope::SymbolKind::FunctionBlock { .. } => {
                if matches_prefix(&sym.name, prefix) {
                    items.push(CompletionItem {
                        label: sym.name.clone(),
                        kind: Some(CompletionItemKind::CLASS),
                        detail: Some("FUNCTION_BLOCK".to_string()),
                        ..Default::default()
                    });
                }
            }
            scope::SymbolKind::Program { .. } => {
                if matches_prefix(&sym.name, prefix) {
                    items.push(CompletionItem {
                        label: sym.name.clone(),
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some("PROGRAM".to_string()),
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
    }
}

fn add_types(doc: &Document, prefix: &str, items: &mut Vec<CompletionItem>) {
    let global = doc.analysis.symbols.scope(doc.analysis.symbols.global_scope_id());
    for sym in global.symbols() {
        if matches!(sym.kind, scope::SymbolKind::Type) && matches_prefix(&sym.name, prefix) {
            items.push(CompletionItem {
                label: sym.name.clone(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some(format!("TYPE {}", sym.ty.display_name())),
                ..Default::default()
            });
        }
    }
}

fn add_keywords(prefix: &str, items: &mut Vec<CompletionItem>) {
    for (kw, snippet) in KEYWORDS {
        if matches_prefix(kw, prefix) {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some(snippet.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
        }
    }
}

fn add_elementary_types(prefix: &str, items: &mut Vec<CompletionItem>) {
    for ty in ELEMENTARY_TYPES {
        if matches_prefix(ty, prefix) {
            items.push(CompletionItem {
                label: ty.to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                ..Default::default()
            });
        }
    }
}

/// Find the scope that contains the given offset.
/// Falls back to the nearest POU scope if the offset is between POUs
/// (e.g. due to parse errors truncating a POU's range).
fn find_scope_for_offset(doc: &Document, offset: usize) -> ScopeId {
    let scopes = doc.analysis.symbols.scopes();
    let global = doc.analysis.symbols.global_scope_id();

    // First try exact range containment
    let mut best_candidate: Option<(&str, usize)> = None;
    for item in &doc.ast.items {
        let (range, name) = match item {
            st_syntax::ast::TopLevelItem::Program(p) => (p.range, &p.name.name),
            st_syntax::ast::TopLevelItem::Function(f) => (f.range, &f.name.name),
            st_syntax::ast::TopLevelItem::FunctionBlock(fb) => (fb.range, &fb.name.name),
            _ => continue,
        };
        if range.start <= offset && offset <= range.end {
            // Exact match
            for scope in scopes {
                if scope.name.eq_ignore_ascii_case(name) && scope.id != global {
                    return scope.id;
                }
            }
        }
        // Track nearest POU that starts before this offset (for fallback)
        if range.start <= offset
            && (best_candidate.is_none() || range.start > best_candidate.unwrap().1) {
                best_candidate = Some((name.as_str(), range.start));
            }
    }

    // Fallback: use the nearest POU that starts before offset
    if let Some((name, _)) = best_candidate {
        for scope in scopes {
            if scope.name.eq_ignore_ascii_case(name) && scope.id != global {
                return scope.id;
            }
        }
    }

    global
}

fn get_word_prefix(source: &str, offset: usize) -> String {
    let bytes = source.as_bytes();
    let mut start = offset;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    std::str::from_utf8(&bytes[start..offset])
        .unwrap_or("")
        .to_string()
}

fn matches_prefix(name: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    name.to_uppercase().starts_with(&prefix.to_uppercase())
}
