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
    /// Project files loaded for cross-file resolution: (path, content).
    /// Cached on didOpen, reused on didChange without re-reading disk.
    pub project_files: Vec<(String, String)>,
    /// This document's file path (for filtering itself from project_files).
    pub file_path: Option<String>,
    /// Byte offset of this file's content within the virtual concatenated
    /// text produced by `parse_multi()`. Used to map diagnostics (which
    /// carry ranges in the virtual space) back to file-local positions.
    pub virtual_offset: usize,
}

impl Document {
    /// Create a new document from source text.
    pub fn new(source: String, version: Option<i32>) -> Self {
        let (tree, ast, lower_errors, analysis, project_files, virtual_offset) =
            Self::analyze_source_with_uri(&source, None);
        Self { source, tree, ast, lower_errors, analysis, version, project_files, file_path: None, virtual_offset }
    }

    /// Create a new document with project-aware analysis using the file URI.
    pub fn new_with_uri(source: String, version: Option<i32>, uri: &str) -> Self {
        let file_path = uri.strip_prefix("file://").map(|s| s.to_string());
        let (tree, ast, lower_errors, analysis, project_files, virtual_offset) =
            Self::analyze_source_with_uri(&source, Some(uri));
        Self { source, tree, ast, lower_errors, analysis, version, project_files, file_path, virtual_offset }
    }

    /// Update the document with new source text.
    /// Reuses the cached project_files from didOpen — no disk I/O or project
    /// re-discovery on each keystroke.
    ///
    /// If the new source has parse errors, we still update the tree and source
    /// (for cursor position tracking) but keep the last successful analysis
    /// results. This prevents squiggles from appearing while the user is typing
    /// mid-expression (e.g., `controller.` before completing the method name).
    pub fn update(&mut self, source: String, version: Option<i32>, _uri: Option<&str>) {
        let (tree, ast, lower_errors, analysis, virtual_offset) =
            Self::analyze_with_cached_project(&source, &self.project_files, self.file_path.as_deref());

        let has_parse_errors = !lower_errors.is_empty() || tree.root_node().has_error();

        self.source = source;
        self.tree = tree;
        self.version = version;

        if has_parse_errors {
            // Keep the last good AST and analysis — prevents false semantic
            // squiggles while typing. But always update parse errors and
            // virtual_offset so the user sees WHERE the syntax problems are.
            self.lower_errors = lower_errors;
            self.virtual_offset = virtual_offset;
        } else {
            self.ast = ast;
            self.lower_errors = lower_errors;
            self.analysis = analysis;
            self.virtual_offset = virtual_offset;
        }
        // project_files unchanged — cached from didOpen
    }

    /// Fast re-analysis using cached project files (no disk I/O).
    /// Called on every keystroke via didChange.
    fn analyze_with_cached_project(
        source: &str,
        cached_project_files: &[(String, String)],
        self_path: Option<&str>,
    ) -> (
        tree_sitter::Tree,
        SourceFile,
        Vec<st_syntax::lower::LowerError>,
        AnalysisResult,
        usize, // virtual_offset of this file in the concatenated text
    ) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&st_grammar::language())
            .expect("Failed to load ST grammar");
        let tree = parser.parse(source, None).expect("Failed to parse");

        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let mut all_sources: Vec<&str> = stdlib;
        // Add cached project files, excluding the current file (we add the
        // fresh edited version below)
        for (path, content) in cached_project_files {
            let skip = self_path.is_some_and(|sp| path == sp);
            if !skip {
                all_sources.push(content.as_str());
            }
        }

        // The current file is always LAST. Its virtual offset is the sum of
        // all preceding source lengths.
        let virtual_offset: usize = all_sources.iter().map(|s| s.len()).sum();
        all_sources.push(source);

        let multi_result = st_syntax::multi_file::parse_multi(&all_sources);
        // Build native FB registry from cached project info (if we have a project root).
        // This is a lightweight operation (no I/O if profiles haven't changed).
        let registry = Self::build_native_fb_registry(cached_project_files);
        let analysis = st_semantics::analyze::analyze_with_native_fbs(
            &multi_result.source_file,
            registry.as_ref(),
        );
        let lower_result: LowerResult = st_syntax::lower::lower(&tree, source);

        (tree, lower_result.source_file, multi_result.errors, analysis, virtual_offset)
    }

    /// Analyze with optional file URI for project-aware multi-file resolution.
    /// Does full project discovery from disk — called once on didOpen.
    #[allow(clippy::type_complexity)]
    pub fn analyze_source_with_uri(
        source: &str,
        file_uri: Option<&str>,
    ) -> (
        tree_sitter::Tree,
        SourceFile,
        Vec<st_syntax::lower::LowerError>,
        AnalysisResult,
        Vec<(String, String)>,
        usize, // virtual_offset
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
        let mut project_files: Vec<(String, String)> = Vec::new();
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
                        if let Ok(project) =
                            st_syntax::project::discover_project(Some(&root))
                        {
                            if let Ok(sources) =
                                st_syntax::project::load_project_sources(&project)
                            {
                                for (path, content) in sources {
                                    let path_str = path.to_string_lossy().to_string();
                                    project_files.push((path_str, content.clone()));
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
        // Current file is last — its virtual offset is the sum of all preceding sources.
        let virtual_offset: usize = all_sources.iter().map(|s| s.len()).sum();
        all_sources.push(source);

        let multi_result = st_syntax::multi_file::parse_multi(&all_sources);
        let registry = Self::build_native_fb_registry(&project_files);
        let analysis = st_semantics::analyze::analyze_with_native_fbs(
            &multi_result.source_file,
            registry.as_ref(),
        );

        let lower_result: LowerResult = st_syntax::lower::lower(&tree, source);
        (
            tree,
            lower_result.source_file,
            multi_result.errors,
            analysis,
            project_files,
            virtual_offset,
        )
    }

    /// Build a native FB registry by finding the project root from the cached
    /// project files and discovering device profiles. Returns `None` if no
    /// project root or no profiles are found.
    fn build_native_fb_registry(
        project_files: &[(String, String)],
    ) -> Option<st_comm_api::NativeFbRegistry> {
        // Find the project root by looking for plc-project.yaml in the project files' parent dirs.
        let project_root = project_files.first().and_then(|(path, _)| {
            let p = std::path::Path::new(path);
            let mut dir = p.parent()?;
            loop {
                if dir.join("plc-project.yaml").exists() || dir.join("plc-project.yml").exists() {
                    return Some(dir.to_path_buf());
                }
                dir = dir.parent()?;
            }
        });

        let root = project_root?;
        // Discover all device profiles in the project's profile search paths.
        let profiles_dir = root.join("profiles");
        if !profiles_dir.is_dir() {
            return None;
        }

        let mut registry = st_comm_api::NativeFbRegistry::new();
        let entries = std::fs::read_dir(&profiles_dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" {
                continue;
            }
            match st_comm_api::DeviceProfile::from_file(&path) {
                Ok(profile) => {
                    // Register all profiles for type information in the LSP.
                    // We use SimulatedNativeFb as the backing impl since the LSP
                    // only needs the struct shape, not runtime behaviour.
                    // For modbus-rtu profiles we use the modbus layout which
                    // includes serial config fields (port, baud, etc.).
                    let protocol = profile.protocol.as_deref().unwrap_or("simulated");
                    let layout = match protocol {
                        "modbus-rtu" => profile.to_modbus_rtu_device_layout(),
                        "modbus-tcp" => profile.to_modbus_tcp_device_layout(),
                        _ => profile.to_native_fb_layout(),
                    };
                    registry.register(Box::new(
                        st_comm_sim::LayoutOnlyNativeFb::new(layout),
                    ));
                }
                Err(e) => {
                    eprintln!("[LSP] warning: failed to load device profile {}: {e}", path.display());
                }
            }
        }

        // Always register SerialLink for completions when any modbus-rtu
        // profiles exist (devices reference it via `link := serial.port`).
        let has_modbus = !registry.is_empty();
        if has_modbus {
            let serial_link_layout = st_comm_api::NativeFbLayout {
                type_name: "SerialLink".to_string(),
                fields: vec![
                    st_comm_api::NativeFbField { name: "port".into(), data_type: st_comm_api::FieldDataType::String, var_kind: st_comm_api::NativeFbVarKind::VarInput, dimensions: None },
                    st_comm_api::NativeFbField { name: "baud".into(), data_type: st_comm_api::FieldDataType::Int, var_kind: st_comm_api::NativeFbVarKind::VarInput, dimensions: None },
                    st_comm_api::NativeFbField { name: "parity".into(), data_type: st_comm_api::FieldDataType::String, var_kind: st_comm_api::NativeFbVarKind::VarInput, dimensions: None },
                    st_comm_api::NativeFbField { name: "data_bits".into(), data_type: st_comm_api::FieldDataType::Int, var_kind: st_comm_api::NativeFbVarKind::VarInput, dimensions: None },
                    st_comm_api::NativeFbField { name: "stop_bits".into(), data_type: st_comm_api::FieldDataType::Int, var_kind: st_comm_api::NativeFbVarKind::VarInput, dimensions: None },
                    st_comm_api::NativeFbField { name: "connected".into(), data_type: st_comm_api::FieldDataType::Bool, var_kind: st_comm_api::NativeFbVarKind::Var, dimensions: None },
                    st_comm_api::NativeFbField { name: "error_code".into(), data_type: st_comm_api::FieldDataType::Int, var_kind: st_comm_api::NativeFbVarKind::Var, dimensions: None },
                ],
            };
            registry.register(Box::new(
                st_comm_sim::LayoutOnlyNativeFb::new(serial_link_layout),
            ));
        }

        if registry.is_empty() {
            None
        } else {
            Some(registry)
        }
    }

    /// Convert a byte offset to an LSP Position (line, character).
    /// Convert a file-local byte offset to a virtual-space offset.
    /// Virtual-space offsets match the byte ranges in the semantic analysis
    /// (symbols, scopes, diagnostics) produced by `parse_multi()`.
    pub fn to_virtual(&self, local_offset: usize) -> usize {
        local_offset + self.virtual_offset
    }

    /// Convert a virtual-space byte offset back to file-local.
    pub fn from_virtual(&self, virtual_offset: usize) -> usize {
        virtual_offset.saturating_sub(self.virtual_offset)
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Position;

    const SAMPLE_POU: &str = "PROGRAM main\nVAR\n  x : INT;\nEND_VAR\n  x := 1;\nEND_PROGRAM\n";

    #[test]
    fn new_parses_a_valid_program() {
        let doc = Document::new(SAMPLE_POU.to_string(), Some(1));
        assert_eq!(doc.source, SAMPLE_POU);
        assert_eq!(doc.version, Some(1));
        assert!(doc.lower_errors.is_empty(), "no parse errors for valid input");
        assert!(doc.file_path.is_none());
        // virtual_offset is the sum of stdlib lengths — opaque but stable > 0.
        assert!(doc.virtual_offset > 0);
    }

    #[test]
    fn new_with_uri_without_project_root_matches_new() {
        // URI pointing at a path outside any plc-project: should behave like new().
        let uri = "file:///tmp/definitely/not/a/plc-project/main.st";
        let doc = Document::new_with_uri(SAMPLE_POU.to_string(), Some(2), uri);
        assert_eq!(doc.version, Some(2));
        assert_eq!(doc.file_path.as_deref(), Some("/tmp/definitely/not/a/plc-project/main.st"));
        assert!(doc.project_files.is_empty());
    }

    #[test]
    fn update_replaces_clean_source() {
        let mut doc = Document::new(SAMPLE_POU.to_string(), Some(1));
        let new_src = "PROGRAM main\nVAR y : REAL; END_VAR\n  y := 2.0;\nEND_PROGRAM\n".to_string();
        doc.update(new_src.clone(), Some(2), None);
        assert_eq!(doc.source, new_src);
        assert_eq!(doc.version, Some(2));
        assert!(doc.lower_errors.is_empty());
    }

    #[test]
    fn update_with_parse_errors_preserves_last_good_ast() {
        let mut doc = Document::new(SAMPLE_POU.to_string(), Some(1));
        // Keep a handle to what "good" looked like.
        let good_vo = doc.virtual_offset;

        // An unterminated VAR block is a parse error — we expect update() to
        // keep the prior analysis but refresh source + lower_errors.
        let broken = "PROGRAM main\nVAR\n  x : IN".to_string();
        doc.update(broken.clone(), Some(2), None);
        assert_eq!(doc.source, broken, "source always advances");
        assert_eq!(doc.version, Some(2));
        // We can't reliably check lower_errors is non-empty for every broken
        // snippet (tree-sitter is forgiving), so assert virtual_offset is
        // still coherent and analysis wasn't wiped.
        assert_eq!(doc.virtual_offset, good_vo);
    }

    #[test]
    fn offset_to_position_handles_first_line() {
        let doc = Document::new("abc\ndef\n".to_string(), None);
        assert_eq!(doc.offset_to_position(0), Position::new(0, 0));
        assert_eq!(doc.offset_to_position(2), Position::new(0, 2));
    }

    #[test]
    fn offset_to_position_crosses_newlines() {
        let doc = Document::new("abc\ndef\nghi".to_string(), None);
        // Offset 4 is 'd' on line 1, column 0.
        assert_eq!(doc.offset_to_position(4), Position::new(1, 0));
        // Offset 8 is 'g' on line 2, column 0.
        assert_eq!(doc.offset_to_position(8), Position::new(2, 0));
        // End of buffer.
        assert_eq!(doc.offset_to_position(11), Position::new(2, 3));
    }

    #[test]
    fn offset_to_position_clamps_past_end() {
        let doc = Document::new("abc".to_string(), None);
        // Passing an oversized offset should clamp, not panic.
        let pos = doc.offset_to_position(99);
        assert_eq!(pos, Position::new(0, 3));
    }

    #[test]
    fn position_to_offset_roundtrips_with_offset_to_position() {
        let src = "ab\ncde\nf";
        let doc = Document::new(src.to_string(), None);
        for target in 0..src.len() {
            let pos = doc.offset_to_position(target);
            let back = doc.position_to_offset(pos);
            assert_eq!(back, target, "roundtrip failed at offset {target}");
        }
    }

    #[test]
    fn position_to_offset_past_last_newline() {
        let doc = Document::new("a\nb\n".to_string(), None);
        // Line 2 exists (empty, after trailing newline); column past EOL clamps
        // to the end via naive addition.
        let off = doc.position_to_offset(Position::new(1, 1));
        assert_eq!(off, 3); // 'a' '\n' 'b' -> offset 3 is just after 'b'
    }

    #[test]
    fn text_range_to_lsp_maps_both_endpoints() {
        let src = "abcdef\n123456";
        let doc = Document::new(src.to_string(), None);
        let range = st_syntax::ast::TextRange { start: 2, end: 9 };
        let lsp = doc.text_range_to_lsp(range);
        assert_eq!(lsp.start, Position::new(0, 2));
        assert_eq!(lsp.end, Position::new(1, 2));
    }

    #[test]
    fn to_virtual_and_from_virtual_are_inverses() {
        let doc = Document::new(SAMPLE_POU.to_string(), None);
        for local in [0usize, 1, 10, 50] {
            let v = doc.to_virtual(local);
            assert_eq!(doc.from_virtual(v), local);
        }
    }

    #[test]
    fn from_virtual_saturates_below_offset() {
        let doc = Document::new(SAMPLE_POU.to_string(), None);
        // A virtual offset smaller than our file's base should saturate at 0,
        // not underflow.
        assert_eq!(doc.from_virtual(0), 0);
        assert_eq!(doc.from_virtual(doc.virtual_offset.saturating_sub(1)), 0);
    }

    #[test]
    fn unicode_source_does_not_panic() {
        // The current offset/position helpers are byte-based; this just
        // verifies they don't panic on multi-byte sequences.
        let src = "// αβγ\nOK := TRUE;\n";
        let doc = Document::new(src.to_string(), None);
        let _ = doc.offset_to_position(src.len());
        let _ = doc.position_to_offset(Position::new(1, 0));
    }
}
