//! Per-document state management.
//!
//! Each open document tracks its source text, tree-sitter parse tree,
//! AST, and semantic analysis results.

use st_semantics::analyze::AnalysisResult;
use st_syntax::ast::SourceFile;
use st_syntax::lower::LowerResult;

/// State for a single open document.
pub struct Document {
    pub source: String,
    pub tree: tree_sitter::Tree,
    pub ast: SourceFile,
    pub lower_errors: Vec<st_syntax::lower::LowerError>,
    pub analysis: AnalysisResult,
    pub version: Option<i32>,
}

impl Document {
    /// Create a new document from source text.
    pub fn new(source: String, version: Option<i32>) -> Self {
        let (tree, ast, lower_errors, analysis) = Self::analyze_source(&source);
        Self {
            source,
            tree,
            ast,
            lower_errors,
            analysis,
            version,
        }
    }

    /// Update the document with new source text.
    pub fn update(&mut self, source: String, version: Option<i32>) {
        let (tree, ast, lower_errors, analysis) = Self::analyze_source(&source);
        self.source = source;
        self.tree = tree;
        self.ast = ast;
        self.lower_errors = lower_errors;
        self.analysis = analysis;
        self.version = version;
    }

    fn analyze_source(
        source: &str,
    ) -> (
        tree_sitter::Tree,
        SourceFile,
        Vec<st_syntax::lower::LowerError>,
        AnalysisResult,
    ) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&st_grammar::language())
            .expect("Failed to load ST grammar");
        let tree = parser.parse(source, None).expect("Failed to parse");

        // Parse with stdlib context for cross-file type/POU resolution
        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let mut all_sources: Vec<&str> = stdlib;
        all_sources.push(source);
        let multi_result = st_syntax::multi_file::parse_multi(&all_sources);

        let analysis = st_semantics::analyze::analyze(&multi_result.source_file);

        // Return the user's AST (from the single-file parse) but with
        // the multi-file analysis (which includes stdlib symbols)
        let lower_result: LowerResult = st_syntax::lower::lower(&tree, source);
        (
            tree,
            lower_result.source_file,
            multi_result.errors,
            analysis,
        )
    }

    /// Convert a byte offset to an LSP Position (line, character).
    pub fn offset_to_position(&self, offset: usize) -> tower_lsp::lsp_types::Position {
        let src = self.source.as_bytes();
        let offset = offset.min(src.len());
        let mut line = 0u32;
        let mut col = 0u32;
        for &b in &src[..offset] {
            if b == b'\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        tower_lsp::lsp_types::Position::new(line, col)
    }

    /// Convert a TextRange to an LSP Range.
    pub fn text_range_to_lsp(&self, range: st_syntax::ast::TextRange) -> tower_lsp::lsp_types::Range {
        tower_lsp::lsp_types::Range::new(
            self.offset_to_position(range.start),
            self.offset_to_position(range.end),
        )
    }

    /// Convert an LSP Position to a byte offset.
    pub fn position_to_offset(&self, pos: tower_lsp::lsp_types::Position) -> usize {
        let mut line = 0u32;
        let mut offset = 0usize;
        let bytes = self.source.as_bytes();
        while offset < bytes.len() && line < pos.line {
            if bytes[offset] == b'\n' {
                line += 1;
            }
            offset += 1;
        }
        offset + pos.character as usize
    }
}
