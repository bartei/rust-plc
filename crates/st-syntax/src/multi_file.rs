//! Multi-file source merging.
//!
//! Parses multiple ST source files and merges their ASTs into a single
//! compilation unit. Used to include the standard library.

use crate::ast::*;
use crate::lower::LowerResult;

/// Parse multiple source strings and merge into a single SourceFile.
/// Items from earlier sources appear first (stdlib before user code).
///
/// All byte ranges in the merged AST are adjusted to a virtual
/// "concatenated" coordinate system: file 0 keeps its original ranges,
/// file 1's ranges are offset by `len(file_0)`, file 2 by
/// `len(file_0) + len(file_1)`, etc. This ensures that items from
/// different files NEVER have overlapping ranges, which is critical
/// for mapping diagnostics back to the correct source file.
pub fn parse_multi(sources: &[&str]) -> LowerResult {
    let mut all_items = Vec::new();
    let mut all_errors = Vec::new();
    let mut cumulative_offset: usize = 0;

    for (i, source) in sources.iter().enumerate() {
        let result = crate::parse(source);

        if cumulative_offset > 0 {
            // Shift all items' byte ranges by the cumulative offset so they
            // don't overlap with items from earlier files.
            for mut item in result.source_file.items {
                offset_top_level_item(&mut item, cumulative_offset);
                all_items.push(item);
            }
        } else {
            all_items.extend(result.source_file.items);
        }

        // Only report errors from user sources (not stdlib), and shift
        // their ranges too so they land in the right virtual position.
        let is_stdlib = i < sources.len().saturating_sub(1)
            && sources.len() > 1
            && i < crate::multi_file::builtin_stdlib().len();
        if !is_stdlib {
            for mut err in result.errors {
                err.range.start += cumulative_offset;
                err.range.end += cumulative_offset;
                all_errors.push(err);
            }
        }

        cumulative_offset += source.len();
    }

    LowerResult {
        source_file: SourceFile {
            items: all_items,
            range: TextRange::new(0, cumulative_offset),
        },
        errors: all_errors,
    }
}

/// Shift every `TextRange` inside a `TopLevelItem` by `offset` bytes.
fn offset_top_level_item(item: &mut TopLevelItem, offset: usize) {
    match item {
        TopLevelItem::Program(p) => {
            shift(&mut p.range, offset);
            shift(&mut p.name.range, offset);
            for vb in &mut p.var_blocks { offset_var_block(vb, offset); }
            for s in &mut p.body { offset_statement(s, offset); }
        }
        TopLevelItem::Function(f) => {
            shift(&mut f.range, offset);
            shift(&mut f.name.range, offset);
            for vb in &mut f.var_blocks { offset_var_block(vb, offset); }
            for s in &mut f.body { offset_statement(s, offset); }
        }
        TopLevelItem::FunctionBlock(fb) => {
            shift(&mut fb.range, offset);
            shift(&mut fb.name.range, offset);
            for vb in &mut fb.var_blocks { offset_var_block(vb, offset); }
            for s in &mut fb.body { offset_statement(s, offset); }
        }
        TopLevelItem::Class(cls) => {
            shift(&mut cls.range, offset);
            shift(&mut cls.name.range, offset);
            for vb in &mut cls.var_blocks { offset_var_block(vb, offset); }
            for m in &mut cls.methods { offset_method(m, offset); }
        }
        TopLevelItem::Interface(iface) => {
            shift(&mut iface.range, offset);
            shift(&mut iface.name.range, offset);
        }
        TopLevelItem::TypeDeclaration(td) => {
            shift(&mut td.range, offset);
            for def in &mut td.definitions {
                offset_type_def(def, offset);
            }
        }
        TopLevelItem::GlobalVarDeclaration(vb) => {
            offset_var_block(vb, offset);
        }
    }
}

fn offset_method(m: &mut MethodDecl, offset: usize) {
    shift(&mut m.range, offset);
    shift(&mut m.name.range, offset);
    for vb in &mut m.var_blocks { offset_var_block(vb, offset); }
    for s in &mut m.body { offset_statement(s, offset); }
}

fn offset_var_block(vb: &mut VarBlock, offset: usize) {
    shift(&mut vb.range, offset);
    for decl in &mut vb.declarations {
        shift(&mut decl.range, offset);
        for name in &mut decl.names { shift(&mut name.range, offset); }
        if let Some(init) = &mut decl.initial_value {
            offset_expression(init, offset);
        }
    }
}

fn offset_statement(s: &mut Statement, offset: usize) {
    match s {
        Statement::Assignment(a) => {
            shift(&mut a.range, offset);
            offset_var_access(&mut a.target, offset);
            offset_expression(&mut a.value, offset);
        }
        Statement::FunctionCall(fc) => offset_func_call(fc, offset),
        Statement::If(i) => {
            shift(&mut i.range, offset);
            offset_expression(&mut i.condition, offset);
            for s in &mut i.then_body { offset_statement(s, offset); }
            for clause in &mut i.elsif_clauses {
                shift(&mut clause.range, offset);
                offset_expression(&mut clause.condition, offset);
                for s in &mut clause.body { offset_statement(s, offset); }
            }
            if let Some(els) = &mut i.else_body {
                for s in els { offset_statement(s, offset); }
            }
        }
        Statement::Case(c) => {
            shift(&mut c.range, offset);
            offset_expression(&mut c.expression, offset);
            for branch in &mut c.branches {
                shift(&mut branch.range, offset);
                for sel in &mut branch.selectors {
                    match sel {
                        CaseSelector::Single(e) => offset_expression(e, offset),
                        CaseSelector::Range(a, b) => {
                            offset_expression(a, offset);
                            offset_expression(b, offset);
                        }
                    }
                }
                for s in &mut branch.body { offset_statement(s, offset); }
            }
            if let Some(els) = &mut c.else_body {
                for s in els { offset_statement(s, offset); }
            }
        }
        Statement::For(f) => {
            shift(&mut f.range, offset);
            shift(&mut f.variable.range, offset);
            offset_expression(&mut f.from, offset);
            offset_expression(&mut f.to, offset);
            if let Some(by) = &mut f.by { offset_expression(by, offset); }
            for s in &mut f.body { offset_statement(s, offset); }
        }
        Statement::While(w) => {
            shift(&mut w.range, offset);
            offset_expression(&mut w.condition, offset);
            for s in &mut w.body { offset_statement(s, offset); }
        }
        Statement::Repeat(r) => {
            shift(&mut r.range, offset);
            offset_expression(&mut r.condition, offset);
            for s in &mut r.body { offset_statement(s, offset); }
        }
        Statement::Return(r) | Statement::Exit(r) | Statement::Empty(r) => {
            shift(r, offset);
        }
    }
}

fn offset_expression(e: &mut Expression, offset: usize) {
    match e {
        Expression::Literal(l) => shift(&mut l.range, offset),
        Expression::Variable(va) => offset_var_access(va, offset),
        Expression::FunctionCall(fc) => offset_func_call(fc, offset),
        Expression::Unary(u) => {
            shift(&mut u.range, offset);
            offset_expression(&mut u.operand, offset);
        }
        Expression::Binary(b) => {
            shift(&mut b.range, offset);
            offset_expression(&mut b.left, offset);
            offset_expression(&mut b.right, offset);
        }
        Expression::Parenthesized(inner) => offset_expression(inner, offset),
        Expression::This(r) | Expression::Super(r) => shift(r, offset),
    }
}

fn offset_var_access(va: &mut VariableAccess, offset: usize) {
    shift(&mut va.range, offset);
    for part in &mut va.parts {
        match part {
            AccessPart::Identifier(id) => shift(&mut id.range, offset),
            AccessPart::Index(exprs) => {
                for e in exprs { offset_expression(e, offset); }
            }
            _ => {}
        }
    }
}

fn offset_func_call(fc: &mut FunctionCallExpr, offset: usize) {
    shift(&mut fc.range, offset);
    for part in &mut fc.name.parts {
        shift(&mut part.range, offset);
    }
    for arg in &mut fc.arguments {
        match arg {
            Argument::Positional(e) => offset_expression(e, offset),
            Argument::Named { name, value } => {
                shift(&mut name.range, offset);
                offset_expression(value, offset);
            }
        }
    }
}

fn offset_type_def(def: &mut TypeDefinition, offset: usize) {
    shift(&mut def.range, offset);
    shift(&mut def.name.range, offset);
}

fn shift(range: &mut TextRange, offset: usize) {
    range.start += offset;
    range.end += offset;
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
        include_str!("../../../stdlib/strings.st"),
    ]
}
