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

    /// Create a new document with project-aware analysis using the file URI.
    pub fn new_with_uri(source: String, version: Option<i32>, uri: &str) -> Self {
        let (tree, ast, lower_errors, analysis) =
            Self::analyze_source_with_uri(&source, Some(uri));
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
        Self::analyze_source_with_uri(source, None)
    }

    /// Analyze with optional file URI for project-aware multi-file resolution.
    pub fn analyze_source_with_uri(
        source: &str,
        file_uri: Option<&str>,
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

        // Collect all sources: stdlib + project siblings (if in a project) + this file
        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let mut all_sources: Vec<&str> = stdlib;

        // Try to discover project context from file path
        let mut project_sources: Vec<String> = Vec::new();
        if let Some(uri) = file_uri {
            if let Some(file_path) = uri.strip_prefix("file://") {
                let file_path = std::path::Path::new(file_path);
                if let Some(dir) = file_path.parent() {
                    // Walk up to find plc-project.yaml
                    let mut check = dir.to_path_buf();
                    let mut project_root = None;
                    loop {
                        if check.join("plc-project.yaml").exists()
                            || check.join("plc-project.yml").exists()
                        {
                            project_root = Some(check.clone());
                            break;
                        }
                        if !check.pop() {
                            break;
                        }
                    }
                    if let Some(root) = project_root {
                        // Load all project files except the current one (we add it separately)
                        if let Ok(project) =
                            st_syntax::project::discover_project(Some(&root))
                        {
                            if let Ok(sources) =
                                st_syntax::project::load_project_sources(&project)
                            {
                                for (path, content) in sources {
                                    // Skip the current file to avoid double-including
                                    if path == file_path {
                                        continue;
                                    }
                                    project_sources.push(content);
                                }
                            }
                        }
                    }
                }
            }
        }

        for s in &project_sources {
            all_sources.push(s.as_str());
        }
        all_sources.push(source);

        let multi_result = st_syntax::multi_file::parse_multi(&all_sources);
        let analysis = st_semantics::analyze::analyze(&multi_result.source_file);

        // Return the user's AST (from the single-file parse) but with
        // the multi-file analysis (which includes project + stdlib symbols)
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
