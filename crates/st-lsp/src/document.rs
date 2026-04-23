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
