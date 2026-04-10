//! LSP server implementation using tower-lsp.

use crate::completion;
use crate::document::Document;
use crate::semantic_tokens::{LEGEND, TokenBuilder};
use tower_lsp::lsp_types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::{Client, LanguageServer};

/// The LSP backend that holds all state.
pub struct Backend {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, Document>>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish diagnostics for a document.
    async fn publish_diagnostics(&self, uri: &Url, doc: &Document) {
        let mut diags = Vec::new();

        // Parse / lower errors — these ranges are in virtual concatenated
        // space (from parse_multi), so we filter to this file's slice and
        // convert to file-local offsets, same as semantic diagnostics below.
        let file_start = doc.virtual_offset;
        let file_end = file_start + doc.source.len();
        for err in &doc.lower_errors {
            if err.range.start < file_start || err.range.start > file_end {
                continue; // belongs to a different file
            }
            let local_range = st_syntax::ast::TextRange::new(
                err.range.start.saturating_sub(file_start),
                err.range.end.saturating_sub(file_start).min(doc.source.len()),
            );
            diags.push(Diagnostic {
                range: doc.text_range_to_lsp(local_range),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("st".to_string()),
                message: err.message.clone(),
                ..Default::default()
            });
        }

        // Semantic diagnostics — only include diagnostics that originate from
        // THIS file's virtual slice [file_start, file_end).
        for d in &doc.analysis.diagnostics {
            if d.range.start < file_start || d.range.end > file_end {
                continue;
            }
            // Convert from virtual-concatenated offset to file-local offset
            // so the LSP range points at the right line:col in this file.
            let local_range = st_syntax::ast::TextRange::new(
                d.range.start - file_start,
                d.range.end - file_start,
            );
            let severity = match d.severity {
                st_semantics::diagnostic::Severity::Error => DiagnosticSeverity::ERROR,
                st_semantics::diagnostic::Severity::Warning => DiagnosticSeverity::WARNING,
                st_semantics::diagnostic::Severity::Info => DiagnosticSeverity::INFORMATION,
            };
            diags.push(Diagnostic {
                range: doc.text_range_to_lsp(local_range),
                severity: Some(severity),
                source: Some("st".to_string()),
                code: Some(NumberOrString::String(format!("{:?}", d.code))),
                message: d.message.clone(),
                ..Default::default()
            });
        }

        self.client
            .publish_diagnostics(uri.clone(), diags, doc.version)
            .await;
    }

    /// Find the symbol at a given offset.
    #[allow(dead_code)]
    fn find_symbol_at_offset<'a>(
        &self,
        doc: &'a Document,
        offset: usize,
    ) -> Option<(&'a st_semantics::scope::Symbol, st_semantics::scope::ScopeId)> {
        // Walk all scopes to find a symbol whose range contains the offset
        for scope in doc.analysis.symbols.scopes() {
            for sym in scope.symbols() {
                if sym.range.start <= offset && offset <= sym.range.end {
                    return Some((sym, scope.id));
                }
            }
        }
        None
    }

    /// Find the identifier at a position and resolve it in the appropriate scope.
    fn resolve_at_position(
        &self,
        doc: &Document,
        offset: usize,
    ) -> Option<(String, st_semantics::scope::ScopeId)> {
        // Find which word the cursor is on
        let bytes = doc.source.as_bytes();
        if offset >= bytes.len() {
            return None;
        }

        // Find word boundaries
        let mut start = offset;
        while start > 0
            && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_')
        {
            start -= 1;
        }
        let mut end = offset;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
        {
            end += 1;
        }

        if start == end {
            return None;
        }

        let word = std::str::from_utf8(&bytes[start..end]).ok()?;

        // Find the scope that contains this offset
        let scope_id = self.find_scope_for_offset(doc, offset);

        // Try to resolve in that scope
        if doc.analysis.symbols.resolve(scope_id, word).is_some() {
            Some((word.to_string(), scope_id))
        } else {
            None
        }
    }

    /// Resolve a symbol's byte range (in VIRTUAL space, from the semantic
    /// analysis) to a Location in a cross-file project. Computes each
    /// project file's virtual offset on the fly and checks if sym_range
    /// falls within that file's slice of the virtual space.
    fn resolve_cross_file_location(
        &self,
        doc: &Document,
        sym_range: st_syntax::ast::TextRange,
    ) -> Option<Location> {
        // Compute per-file virtual offsets using the same algorithm as
        // parse_multi / analyze_with_cached_project.
        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let stdlib_len: usize = stdlib.iter().map(|s| s.len()).sum();
        let mut cumulative = stdlib_len;

        for (path, content) in &doc.project_files {
            let file_start = cumulative;
            let file_end = file_start + content.len();
            cumulative = file_end;

            // Does the symbol fall within this file's virtual slice?
            if sym_range.start < file_start || sym_range.end > file_end {
                continue;
            }

            // Convert to file-local offsets
            let local_start = sym_range.start - file_start;
            let local_end = sym_range.end - file_start;

            let file_uri = tower_lsp::lsp_types::Url::from_file_path(path).ok()?;
            let src = content.as_bytes();
            let start_offset = local_start.min(src.len());
            let end_offset = local_end.min(src.len());
            let mut start_line = 0u32;
            let mut start_col = 0u32;
            for &b in &src[..start_offset] {
                if b == b'\n' { start_line += 1; start_col = 0; }
                else { start_col += 1; }
            }
            let mut end_line = start_line;
            let mut end_col = start_col;
            for &b in &src[start_offset..end_offset] {
                if b == b'\n' { end_line += 1; end_col = 0; }
                else { end_col += 1; }
            }
            return Some(Location {
                uri: file_uri,
                range: tower_lsp::lsp_types::Range {
                    start: tower_lsp::lsp_types::Position { line: start_line, character: start_col },
                    end: tower_lsp::lsp_types::Position { line: end_line, character: end_col },
                },
            });
        }
        None
    }

    /// Resolve go-to-definition for a method name after a dot.
    /// E.g., cursor on "Configure" in "controller.Configure()" → find the METHOD
    /// declaration in the class file.
    fn resolve_method_definition(
        &self,
        doc: &Document,
        offset: usize,
    ) -> Option<Location> {
        let bytes = doc.source.as_bytes();
        if offset >= bytes.len() {
            return None;
        }

        // Extract the word under cursor (the method name)
        let mut end = offset;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        let mut start = offset;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        if start == end {
            return None;
        }
        let method_name = std::str::from_utf8(&bytes[start..end]).ok()?;

        // Check if there's a dot before this word
        let before_word = start.checked_sub(1)?;
        if bytes[before_word] != b'.' {
            return None;
        }

        // Extract the object name before the dot
        let dot_pos = before_word;
        let obj_end = dot_pos;
        let mut obj_start = obj_end;
        while obj_start > 0 && (bytes[obj_start - 1].is_ascii_alphanumeric() || bytes[obj_start - 1] == b'_') {
            obj_start -= 1;
        }
        if obj_start == obj_end {
            return None;
        }
        let obj_name = std::str::from_utf8(&bytes[obj_start..obj_end]).ok()?;

        // Resolve the object's type
        let scope_id = self.find_scope_for_offset(doc, dot_pos);
        let (_sid, sym) = doc.analysis.symbols.resolve(scope_id, obj_name)?;

        let class_name = match sym.ty.resolved() {
            st_semantics::types::Ty::Class { name } => name.clone(),
            st_semantics::types::Ty::FunctionBlock { name } => name.clone(),
            _ => return None,
        };

        // Find the method in the class hierarchy by searching project files
        for (path, content) in &doc.project_files {
            let parse = st_syntax::parse(content);
            for item in &parse.source_file.items {
                if let st_syntax::ast::TopLevelItem::Class(cls) = item {
                    // Check this class and its ancestors
                    if cls.name.name.eq_ignore_ascii_case(&class_name) || self.class_has_ancestor(doc, &class_name, &cls.name.name) {
                        for m in &cls.methods {
                            if m.name.name.eq_ignore_ascii_case(method_name) {
                                let file_uri = tower_lsp::lsp_types::Url::from_file_path(path).ok()?;
                                let src = content.as_bytes();
                                let s = m.range.start.min(src.len());
                                let mut line = 0u32;
                                let mut col = 0u32;
                                for &b in &src[..s] {
                                    if b == b'\n' { line += 1; col = 0; } else { col += 1; }
                                }
                                return Some(Location {
                                    uri: file_uri,
                                    range: tower_lsp::lsp_types::Range {
                                        start: tower_lsp::lsp_types::Position { line, character: col },
                                        end: tower_lsp::lsp_types::Position { line, character: col },
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Check if `class_name` has `ancestor_name` in its inheritance chain.
    fn class_has_ancestor(&self, doc: &Document, class_name: &str, ancestor_name: &str) -> bool {
        let mut current = Some(class_name.to_string());
        while let Some(ref name) = current {
            let base = doc.analysis.symbols.resolve_class(name)
                .and_then(|sym| {
                    if let st_semantics::scope::SymbolKind::Class { base_class, .. } = &sym.kind {
                        base_class.clone()
                    } else {
                        None
                    }
                });
            match base {
                Some(b) if b.eq_ignore_ascii_case(ancestor_name) => return true,
                Some(b) => current = Some(b),
                None => return false,
            }
        }
        false
    }

    /// Find the innermost scope containing the given offset.
    /// Find the scope that contains a FILE-LOCAL byte offset.
    /// Converts to virtual space internally to match the analysis's scope
    /// ranges, then uses the AST (file-local) to map back to a scope name.
    fn find_scope_for_offset(
        &self,
        doc: &Document,
        offset: usize,
    ) -> st_semantics::scope::ScopeId {
        let scopes = doc.analysis.symbols.scopes();
        let global = doc.analysis.symbols.global_scope_id();

        // doc.ast uses file-local ranges. Check which POU the cursor is in.
        for item in &doc.ast.items {
            let (range, name) = match item {
                st_syntax::ast::TopLevelItem::Program(p) => (p.range, &p.name.name),
                st_syntax::ast::TopLevelItem::Function(f) => (f.range, &f.name.name),
                st_syntax::ast::TopLevelItem::FunctionBlock(fb) => (fb.range, &fb.name.name),
                _ => continue,
            };
            if range.start <= offset && offset <= range.end {
                for scope in scopes {
                    if scope.name.eq_ignore_ascii_case(name) && scope.id != global {
                        return scope.id;
                    }
                }
            }
        }
        global
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                document_formatting_provider: Some(OneOf::Left(true)),
                document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
                    first_trigger_character: "\n".to_string(),
                    more_trigger_character: Some(vec![";".to_string()]),
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: LEGEND.clone(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            ..Default::default()
                        },
                    ),
                ),
                document_highlight_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                linked_editing_range_provider: Some(
                    LinkedEditingRangeServerCapabilities::Simple(true),
                ),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        tracing::info!("IEC 61131-3 ST language server initialized");
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let doc = Document::new_with_uri(
            params.text_document.text,
            Some(params.text_document.version),
            uri.as_str(),
        );
        self.publish_diagnostics(&uri, &doc).await;
        self.documents.write().await.insert(uri, doc);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        // Full sync: the last content change is the full text
        if let Some(change) = params.content_changes.into_iter().last() {
            let mut docs = self.documents.write().await;
            if let Some(doc) = docs.get_mut(&uri) {
                doc.update(change.text, Some(params.text_document.version), Some(uri.as_str()));
                self.publish_diagnostics(&uri, doc).await;
            } else {
                let doc = Document::new_with_uri(
                    change.text,
                    Some(params.text_document.version),
                    uri.as_str(),
                );
                self.publish_diagnostics(&uri, &doc).await;
                docs.insert(uri, doc);
            }
        }
    }

    async fn did_save(&self, _params: DidSaveTextDocumentParams) {
        // Content already processed via didChange; nothing extra on save.
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = doc.position_to_offset(pos);

        if let Some((word, scope_id)) = self.resolve_at_position(doc, offset) {
            if let Some((_sid, sym)) =
                doc.analysis.symbols.resolve(scope_id, &word)
            {
                let type_info = sym.ty.display_name();
                let kind_info = match &sym.kind {
                    st_semantics::scope::SymbolKind::Variable(vk) => {
                        format!("{vk:?}")
                    }
                    st_semantics::scope::SymbolKind::Function { return_type, params } => {
                        let param_list = params
                            .iter()
                            .map(|p| format!("{}: {}", p.name, p.ty.display_name()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("FUNCTION({}) : {}", param_list, return_type.display_name())
                    }
                    st_semantics::scope::SymbolKind::FunctionBlock { params, outputs } => {
                        let param_list = params
                            .iter()
                            .map(|p| format!("{}: {}", p.name, p.ty.display_name()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let out_list = outputs
                            .iter()
                            .map(|p| format!("{}: {}", p.name, p.ty.display_name()))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("FUNCTION_BLOCK({param_list}) => ({out_list})")
                    }
                    st_semantics::scope::SymbolKind::Program { .. } => "PROGRAM".to_string(),
                    st_semantics::scope::SymbolKind::Class { methods, .. } => {
                        let method_list = methods
                            .iter()
                            .map(|m| m.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("CLASS (methods: {method_list})")
                    }
                    st_semantics::scope::SymbolKind::Interface { methods, .. } => {
                        let method_list = methods
                            .iter()
                            .map(|m| m.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("INTERFACE (methods: {method_list})")
                    }
                    st_semantics::scope::SymbolKind::Type => format!("TYPE {type_info}"),
                };

                let markdown = format!(
                    "```st\n{} : {}\n```\n---\n{}",
                    sym.name, type_info, kind_info
                );

                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown,
                    }),
                    range: Some(doc.text_range_to_lsp(sym.range)),
                }));
            }
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = doc.position_to_offset(pos);

        // First: check if this is a method name after a dot (e.g., controller.Configure)
        if let Some(location) = self.resolve_method_definition(doc, offset) {
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        // Then: standard symbol resolution
        if let Some((word, scope_id)) = self.resolve_at_position(doc, offset) {
            if let Some((_sid, sym)) =
                doc.analysis.symbols.resolve(scope_id, &word)
            {
                let sym_range = sym.range;

                // sym_range is in virtual space. Try cross-file first.
                if !doc.project_files.is_empty() {
                    if let Some(location) = self.resolve_cross_file_location(doc, sym_range) {
                        return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                    }
                }

                // Fall back to current file: convert virtual → file-local.
                let local_range = st_syntax::ast::TextRange::new(
                    doc.from_virtual(sym_range.start),
                    doc.from_virtual(sym_range.end),
                );
                if local_range.end <= doc.source.len() {
                    let range = doc.text_range_to_lsp(local_range);
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: uri.clone(),
                        range,
                    })));
                }
            }
        }

        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let mut builder = TokenBuilder::new(&doc.source);
        builder.build_from_tree(&doc.tree, doc.source.as_bytes());

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: builder.finish(),
        })))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let trigger = params
            .context
            .as_ref()
            .and_then(|c| c.trigger_character.as_deref());

        let items = completion::completions(doc, pos, trigger);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let mut symbols = Vec::new();
        for item in &doc.ast.items {
            match item {
                st_syntax::ast::TopLevelItem::Program(p) => {
                    let mut children = Vec::new();
                    add_var_symbols(doc, &p.var_blocks, &mut children);
                    #[allow(deprecated)]
                    symbols.push(DocumentSymbol {
                        name: p.name.name.clone(),
                        detail: Some("PROGRAM".to_string()),
                        kind: SymbolKind::MODULE,
                        range: doc.text_range_to_lsp(p.range),
                        selection_range: doc.text_range_to_lsp(p.name.range),
                        children: Some(children),
                        tags: None,
                        deprecated: None,
                    });
                }
                st_syntax::ast::TopLevelItem::Function(f) => {
                    let mut children = Vec::new();
                    add_var_symbols(doc, &f.var_blocks, &mut children);
                    #[allow(deprecated)]
                    symbols.push(DocumentSymbol {
                        name: f.name.name.clone(),
                        detail: Some(format!("FUNCTION : {}", type_display(&f.return_type))),
                        kind: SymbolKind::FUNCTION,
                        range: doc.text_range_to_lsp(f.range),
                        selection_range: doc.text_range_to_lsp(f.name.range),
                        children: Some(children),
                        tags: None,
                        deprecated: None,
                    });
                }
                st_syntax::ast::TopLevelItem::FunctionBlock(fb) => {
                    let mut children = Vec::new();
                    add_var_symbols(doc, &fb.var_blocks, &mut children);
                    #[allow(deprecated)]
                    symbols.push(DocumentSymbol {
                        name: fb.name.name.clone(),
                        detail: Some("FUNCTION_BLOCK".to_string()),
                        kind: SymbolKind::CLASS,
                        range: doc.text_range_to_lsp(fb.range),
                        selection_range: doc.text_range_to_lsp(fb.name.range),
                        children: Some(children),
                        tags: None,
                        deprecated: None,
                    });
                }
                st_syntax::ast::TopLevelItem::Class(cls) => {
                    let mut children = Vec::new();
                    add_var_symbols(doc, &cls.var_blocks, &mut children);
                    for method in &cls.methods {
                        #[allow(deprecated)]
                        children.push(DocumentSymbol {
                            name: method.name.name.clone(),
                            detail: Some("METHOD".to_string()),
                            kind: SymbolKind::METHOD,
                            range: doc.text_range_to_lsp(method.range),
                            selection_range: doc.text_range_to_lsp(method.name.range),
                            children: None,
                            tags: None,
                            deprecated: None,
                        });
                    }
                    #[allow(deprecated)]
                    symbols.push(DocumentSymbol {
                        name: cls.name.name.clone(),
                        detail: Some("CLASS".to_string()),
                        kind: SymbolKind::CLASS,
                        range: doc.text_range_to_lsp(cls.range),
                        selection_range: doc.text_range_to_lsp(cls.name.range),
                        children: Some(children),
                        tags: None,
                        deprecated: None,
                    });
                }
                st_syntax::ast::TopLevelItem::Interface(iface) => {
                    let mut children = Vec::new();
                    for method in &iface.methods {
                        #[allow(deprecated)]
                        children.push(DocumentSymbol {
                            name: method.name.name.clone(),
                            detail: Some("METHOD".to_string()),
                            kind: SymbolKind::METHOD,
                            range: doc.text_range_to_lsp(method.range),
                            selection_range: doc.text_range_to_lsp(method.name.range),
                            children: None,
                            tags: None,
                            deprecated: None,
                        });
                    }
                    #[allow(deprecated)]
                    symbols.push(DocumentSymbol {
                        name: iface.name.name.clone(),
                        detail: Some("INTERFACE".to_string()),
                        kind: SymbolKind::INTERFACE,
                        range: doc.text_range_to_lsp(iface.range),
                        selection_range: doc.text_range_to_lsp(iface.name.range),
                        children: Some(children),
                        tags: None,
                        deprecated: None,
                    });
                }
                st_syntax::ast::TopLevelItem::TypeDeclaration(td) => {
                    for def in &td.definitions {
                        #[allow(deprecated)]
                        symbols.push(DocumentSymbol {
                            name: def.name.name.clone(),
                            detail: Some("TYPE".to_string()),
                            kind: SymbolKind::STRUCT,
                            range: doc.text_range_to_lsp(def.range),
                            selection_range: doc.text_range_to_lsp(def.name.range),
                            children: None,
                            tags: None,
                            deprecated: None,
                        });
                    }
                }
                st_syntax::ast::TopLevelItem::GlobalVarDeclaration(vb) => {
                    for decl in &vb.declarations {
                        for name in &decl.names {
                            #[allow(deprecated)]
                            symbols.push(DocumentSymbol {
                                name: name.name.clone(),
                                detail: Some("VAR_GLOBAL".to_string()),
                                kind: SymbolKind::VARIABLE,
                                range: doc.text_range_to_lsp(decl.range),
                                selection_range: doc.text_range_to_lsp(name.range),
                                children: None,
                                tags: None,
                                deprecated: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    // ── Signature Help ──────────────────────────────────────────────
    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let offset = doc.position_to_offset(pos);
        if let Some((word, scope_id)) = self.resolve_at_position(doc, offset) {
            if let Some((_sid, sym)) = doc.analysis.symbols.resolve(scope_id, &word) {
                match &sym.kind {
                    st_semantics::scope::SymbolKind::Function { return_type, params } => {
                        let param_infos: Vec<ParameterInformation> = params.iter().map(|p| {
                            ParameterInformation {
                                label: ParameterLabel::Simple(format!("{}: {}", p.name, p.ty.display_name())),
                                documentation: None,
                            }
                        }).collect();
                        let sig = SignatureInformation {
                            label: format!("{}({}) : {}",
                                sym.name,
                                params.iter().map(|p| format!("{}: {}", p.name, p.ty.display_name())).collect::<Vec<_>>().join(", "),
                                return_type.display_name()
                            ),
                            documentation: None,
                            parameters: Some(param_infos),
                            active_parameter: None,
                        };
                        return Ok(Some(SignatureHelp {
                            signatures: vec![sig],
                            active_signature: Some(0),
                            active_parameter: None,
                        }));
                    }
                    st_semantics::scope::SymbolKind::FunctionBlock { params, outputs } => {
                        let all_params: Vec<_> = params.iter().chain(outputs.iter()).collect();
                        let param_infos: Vec<ParameterInformation> = all_params.iter().map(|p| {
                            ParameterInformation {
                                label: ParameterLabel::Simple(format!("{}: {}", p.name, p.ty.display_name())),
                                documentation: None,
                            }
                        }).collect();
                        let sig = SignatureInformation {
                            label: format!("{}({})",
                                sym.name,
                                all_params.iter().map(|p| format!("{}: {}", p.name, p.ty.display_name())).collect::<Vec<_>>().join(", "),
                            ),
                            documentation: None,
                            parameters: Some(param_infos),
                            active_parameter: None,
                        };
                        return Ok(Some(SignatureHelp {
                            signatures: vec![sig],
                            active_signature: Some(0),
                            active_parameter: None,
                        }));
                    }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    // ── References ──────────────────────────────────────────────────
    async fn references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let offset = doc.position_to_offset(pos);
        let word = self.get_word_at(doc, offset);
        if word.is_empty() {
            return Ok(None);
        }

        // Find all occurrences of the word in the source
        let mut locations = Vec::new();
        let bytes = doc.source.as_bytes();
        let word_upper = word.to_uppercase();
        let mut search_pos = 0;

        while search_pos < bytes.len() {
            if let Some(idx) = doc.source[search_pos..].to_uppercase().find(&word_upper) {
                let abs_pos = search_pos + idx;
                // Check it's a whole word (not part of a larger identifier)
                let before_ok = abs_pos == 0 || !bytes[abs_pos - 1].is_ascii_alphanumeric() && bytes[abs_pos - 1] != b'_';
                let after_pos = abs_pos + word.len();
                let after_ok = after_pos >= bytes.len() || !bytes[after_pos].is_ascii_alphanumeric() && bytes[after_pos] != b'_';

                if before_ok && after_ok {
                    let start = doc.offset_to_position(abs_pos);
                    let end = doc.offset_to_position(after_pos);
                    locations.push(Location {
                        uri: uri.clone(),
                        range: Range::new(start, end),
                    });
                }
                search_pos = abs_pos + word.len();
            } else {
                break;
            }
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    // ── Rename ──────────────────────────────────────────────────────
    async fn rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let new_name = &params.new_name;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let offset = doc.position_to_offset(pos);
        let word = self.get_word_at(doc, offset);
        if word.is_empty() {
            return Ok(None);
        }

        // Find all occurrences and create text edits
        let mut edits = Vec::new();
        let bytes = doc.source.as_bytes();
        let word_upper = word.to_uppercase();
        let mut search_pos = 0;

        while search_pos < bytes.len() {
            if let Some(idx) = doc.source[search_pos..].to_uppercase().find(&word_upper) {
                let abs_pos = search_pos + idx;
                let before_ok = abs_pos == 0 || !bytes[abs_pos - 1].is_ascii_alphanumeric() && bytes[abs_pos - 1] != b'_';
                let after_pos = abs_pos + word.len();
                let after_ok = after_pos >= bytes.len() || !bytes[after_pos].is_ascii_alphanumeric() && bytes[after_pos] != b'_';

                if before_ok && after_ok {
                    let start = doc.offset_to_position(abs_pos);
                    let end = doc.offset_to_position(after_pos);
                    edits.push(TextEdit {
                        range: Range::new(start, end),
                        new_text: new_name.clone(),
                    });
                }
                search_pos = abs_pos + word.len();
            } else {
                break;
            }
        }

        if edits.is_empty() {
            return Ok(None);
        }

        let mut changes = std::collections::HashMap::new();
        changes.insert(uri.clone(), edits);

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }

    // ── Formatting ──────────────────────────────────────────────────
    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let formatted = format_st_source(&doc.source, params.options.tab_size as usize);
        if formatted == doc.source {
            return Ok(None);
        }

        // Replace the entire document
        let last_line = doc.source.lines().count().saturating_sub(1) as u32;
        let last_col = doc.source.lines().last().map(|l| l.len()).unwrap_or(0) as u32;

        Ok(Some(vec![TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(last_line, last_col)),
            new_text: formatted,
        }]))
    }

    // ── On-Type Formatting (auto-indent after Enter / ;) ────────────
    async fn on_type_formatting(
        &self,
        params: DocumentOnTypeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };
        let tab = " ".repeat(params.options.tab_size as usize);
        let lines: Vec<&str> = doc.source.lines().collect();

        match params.ch.as_str() {
            // After Enter: auto-indent the new (empty) line based on what's
            // on the previous line. This is the highest-value on-type
            // formatting behavior — saves the user from manually pressing
            // Tab or aligning by hand after every THEN, DO, VAR, etc.
            "\n" => {
                let cur_line = pos.line as usize;
                if cur_line == 0 || cur_line >= lines.len() {
                    return Ok(None);
                }

                let prev = lines[cur_line - 1];
                let prev_indent = leading_whitespace(prev);
                let prev_trimmed = prev.trim().to_uppercase();

                // Determine indent adjustment based on the previous line's
                // content. Opening keywords add one level; closing keywords
                // don't change (END_* is already indented correctly).
                let delta = if starts_with_opener(&prev_trimmed) {
                    1i32
                } else {
                    0
                };

                let base_indent_chars = prev_indent.len() as i32;
                let new_indent_chars =
                    (base_indent_chars + delta * tab.len() as i32).max(0) as usize;
                let new_indent = " ".repeat(new_indent_chars);

                // Only emit a TextEdit if the current line's indent is wrong.
                let cur_text = lines.get(cur_line).copied().unwrap_or("");
                let cur_indent = leading_whitespace(cur_text);
                if cur_indent == new_indent {
                    return Ok(None);
                }

                // Replace the current line's leading whitespace.
                let edit_range = Range::new(
                    Position::new(pos.line, 0),
                    Position::new(pos.line, cur_indent.len() as u32),
                );
                Ok(Some(vec![TextEdit {
                    range: edit_range,
                    new_text: new_indent,
                }]))
            }

            // After `;`: reindent the current line. Useful when the user
            // typed a closing keyword + `;` (like `END_IF;`) at the wrong
            // indent level. We compute the correct indent and fix it.
            ";" => {
                let cur_line = pos.line as usize;
                if cur_line >= lines.len() {
                    return Ok(None);
                }

                let cur_text = lines[cur_line];
                let cur_indent = leading_whitespace(cur_text);
                let cur_trimmed = cur_text.trim().to_uppercase();

                // For closing keywords, use the indent of the PREVIOUS non-
                // empty line minus one level (to match the opening keyword).
                if cur_trimmed.starts_with("END_") || cur_trimmed.starts_with("UNTIL ") {
                    // Find the previous non-empty line's indent.
                    let prev_indent = (0..cur_line)
                        .rev()
                        .find_map(|i| {
                            let l = lines[i].trim();
                            if l.is_empty() {
                                None
                            } else {
                                Some(leading_whitespace(lines[i]))
                            }
                        })
                        .unwrap_or("");
                    let expected_chars = prev_indent
                        .len()
                        .saturating_sub(tab.len());
                    let expected = " ".repeat(expected_chars);

                    if cur_indent == expected {
                        return Ok(None);
                    }

                    let edit_range = Range::new(
                        Position::new(pos.line, 0),
                        Position::new(pos.line, cur_indent.len() as u32),
                    );
                    return Ok(Some(vec![TextEdit {
                        range: edit_range,
                        new_text: expected,
                    }]));
                }

                Ok(None)
            }

            _ => Ok(None),
        }
    }

    // ── Linked Editing Range (matching keyword pairs) ────────────────
    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = doc.position_to_offset(pos);
        let source = &doc.source;

        // Find the word at the cursor. If it's a block keyword (IF, FOR,
        // END_IF, VAR, PROGRAM, etc.), find its matching counterpart in the
        // same AST block and return both ranges for simultaneous editing.
        let Some(word_range) = find_word_range(source, offset) else {
            return Ok(None);
        };
        let word = source[word_range.start..word_range.end].to_uppercase();

        // Try to find a keyword pair in the source using the AST to scope
        // the search to the correct nesting level.
        if let Some((open_range, close_range)) =
            find_keyword_pair(&doc.ast, source, offset, &word)
        {
            // Only respond if the cursor is actually ON one of the keywords.
            let on_open = offset >= open_range.start && offset <= open_range.end;
            let on_close = offset >= close_range.start && offset <= close_range.end;
            if on_open || on_close {
                return Ok(Some(LinkedEditingRanges {
                    ranges: vec![
                        doc.text_range_to_lsp(open_range),
                        doc.text_range_to_lsp(close_range),
                    ],
                    word_pattern: None,
                }));
            }
        }

        Ok(None)
    }

    // ── Code Actions ────────────────────────────────────────────────
    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let mut actions = Vec::new();

        for diag in &params.context.diagnostics {
            // Quick fix: declare undeclared variable
            if diag.message.contains("undeclared variable") {
                if let Some(var_name) = diag.message.strip_prefix("undeclared variable '") {
                    let var_name = var_name.trim_end_matches('\'');
                    // Find the VAR block to insert into
                    if let Some(insert_pos) = find_var_block_insert_position(&doc.source) {
                        let indent = "    ";
                        let new_text = format!("{indent}{var_name} : INT := 0;\n");
                        let pos = doc.offset_to_position(insert_pos);

                        let mut changes = std::collections::HashMap::new();
                        changes.insert(uri.clone(), vec![TextEdit {
                            range: Range::new(pos, pos),
                            new_text,
                        }]);

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: format!("Declare '{var_name}' as INT"),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diag.clone()]),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }));
                    }
                }
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    // ── Document Highlight ──────────────────────────────────────────
    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let offset = doc.position_to_offset(pos);
        let word = self.get_word_at(doc, offset);
        if word.is_empty() { return Ok(None); }

        let mut highlights = Vec::new();
        let bytes = doc.source.as_bytes();
        let word_upper = word.to_uppercase();
        let mut search_pos = 0;

        while search_pos < bytes.len() {
            if let Some(idx) = doc.source[search_pos..].to_uppercase().find(&word_upper) {
                let abs_pos = search_pos + idx;
                let before_ok = abs_pos == 0 || !(bytes[abs_pos - 1].is_ascii_alphanumeric() || bytes[abs_pos - 1] == b'_');
                let after_pos = abs_pos + word.len();
                let after_ok = after_pos >= bytes.len() || !(bytes[after_pos].is_ascii_alphanumeric() || bytes[after_pos] == b'_');

                if before_ok && after_ok {
                    highlights.push(DocumentHighlight {
                        range: Range::new(
                            doc.offset_to_position(abs_pos),
                            doc.offset_to_position(after_pos),
                        ),
                        kind: Some(DocumentHighlightKind::TEXT),
                    });
                }
                search_pos = abs_pos + word.len();
            } else {
                break;
            }
        }

        if highlights.is_empty() { Ok(None) } else { Ok(Some(highlights)) }
    }

    // ── Folding Ranges ──────────────────────────────────────────────
    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let mut ranges = Vec::new();
        let mut stack: Vec<(u32, &str)> = Vec::new(); // (start_line, keyword)

        for (line_num, line) in doc.source.lines().enumerate() {
            let trimmed = line.trim().to_uppercase();
            let ln = line_num as u32;

            // Opening keywords
            if trimmed.starts_with("PROGRAM ")
                || trimmed.starts_with("FUNCTION_BLOCK ")
                || trimmed.starts_with("FUNCTION ")
                || trimmed.starts_with("TYPE")
                || trimmed.starts_with("STRUCT")
                || trimmed.starts_with("VAR")
                || trimmed.starts_with("IF ")
                || trimmed.starts_with("FOR ")
                || trimmed.starts_with("WHILE ")
                || trimmed.starts_with("REPEAT")
                || trimmed.starts_with("CASE ")
                || trimmed == "ELSE"
                || trimmed.starts_with("ELSIF ")
            {
                stack.push((ln, "block"));
            }

            // Closing keywords
            if trimmed.starts_with("END_") || trimmed == "ELSE" || trimmed.starts_with("ELSIF ") || trimmed.starts_with("UNTIL ") {
                if let Some((start, _)) = stack.pop() {
                    if ln > start {
                        ranges.push(FoldingRange {
                            start_line: start,
                            start_character: None,
                            end_line: ln,
                            end_character: None,
                            kind: Some(FoldingRangeKind::Region),
                            collapsed_text: None,
                        });
                    }
                }
            }

            // Comment blocks
            if trimmed.starts_with("(*") && !trimmed.contains("*)") {
                stack.push((ln, "comment"));
            }
            if trimmed.contains("*)") && !trimmed.starts_with("(*") {
                if let Some((start, kind)) = stack.pop() {
                    if kind == "comment" && ln > start {
                        ranges.push(FoldingRange {
                            start_line: start,
                            start_character: None,
                            end_line: ln,
                            end_character: None,
                            kind: Some(FoldingRangeKind::Comment),
                            collapsed_text: None,
                        });
                    }
                }
            }
        }

        if ranges.is_empty() { Ok(None) } else { Ok(Some(ranges)) }
    }

    // ── Selection Range (smart expand/shrink selection) ──────────────
    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let mut result = Vec::new();
        for pos in &params.positions {
            let offset = doc.position_to_offset(*pos);
            let ranges = collect_containing_ranges(&doc.ast, offset);
            // Also add the "word at cursor" range as the innermost level
            // by finding the identifier/number token boundaries.
            let word_range = find_word_range(&doc.source, offset);
            let mut all = Vec::new();
            if let Some(wr) = word_range {
                if wr.start < wr.end {
                    all.push(wr);
                }
            }
            for r in &ranges {
                // De-duplicate: don't add if we already have an identical range.
                if !all.iter().any(|existing| existing.start == r.start && existing.end == r.end) {
                    all.push(*r);
                }
            }
            // Sort smallest→largest (innermost first).
            all.sort_by_key(|r| r.end - r.start);

            // Build the chain from innermost to outermost. Each level's
            // `parent` points to the next larger enclosing range.
            let mut chain: Option<SelectionRange> = None;
            for r in all.into_iter().rev() {
                chain = Some(SelectionRange {
                    range: doc.text_range_to_lsp(r),
                    parent: chain.map(Box::new),
                });
            }
            result.push(chain.unwrap_or(SelectionRange {
                range: doc.text_range_to_lsp(st_syntax::ast::TextRange::new(offset, offset)),
                parent: None,
            }));
        }

        Ok(Some(result))
    }

    // ── Inlay Hints (parameter names at call sites) ─────────────────
    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        // Convert the visible range to byte offsets for filtering.
        let range_start = doc.position_to_offset(params.range.start);
        let range_end = doc.position_to_offset(params.range.end);

        let mut hints = Vec::new();

        // Walk the AST to find all function/FB calls in the visible range
        // and emit parameter-name hints for positional arguments.
        for item in &doc.ast.items {
            let (body, _var_blocks, item_range) = match item {
                ast::TopLevelItem::Program(p) => {
                    (p.body.as_slice(), p.var_blocks.as_slice(), p.range)
                }
                ast::TopLevelItem::Function(f) => {
                    (f.body.as_slice(), f.var_blocks.as_slice(), f.range)
                }
                ast::TopLevelItem::FunctionBlock(fb) => {
                    (fb.body.as_slice(), fb.var_blocks.as_slice(), fb.range)
                }
                ast::TopLevelItem::Class(cls) => {
                    // Class methods
                    for method in &cls.methods {
                        if method.range.end < range_start || method.range.start > range_end {
                            continue;
                        }
                        collect_call_hints(
                            &method.body,
                            doc,
                            range_start,
                            range_end,
                            &mut hints,
                        );
                    }
                    continue;
                }
                _ => continue,
            };
            if item_range.end < range_start || item_range.start > range_end {
                continue;
            }
            collect_call_hints(body, doc, range_start, range_end, &mut hints);
        }

        if hints.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hints))
        }
    }

    // ── Call Hierarchy (cross-reference: who calls what) ─────────────
    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = doc.position_to_offset(pos);

        // Find the POU (PROGRAM/FUNCTION/FB/METHOD) at the cursor position.
        for item in &doc.ast.items {
            let (name, kind, item_range, name_range) = match item {
                ast::TopLevelItem::Function(f) => {
                    ("Function", SymbolKind::FUNCTION, f.range, f.name.range)
                }
                ast::TopLevelItem::FunctionBlock(fb) => {
                    ("FunctionBlock", SymbolKind::CLASS, fb.range, fb.name.range)
                }
                ast::TopLevelItem::Program(p) => {
                    ("Program", SymbolKind::MODULE, p.range, p.name.range)
                }
                ast::TopLevelItem::Class(cls) => {
                    // Check if cursor is on a method inside the class
                    for method in &cls.methods {
                        if contains(method.range, offset) {
                            let full_name = format!("{}.{}", cls.name.name, method.name.name);
                            return Ok(Some(vec![CallHierarchyItem {
                                name: full_name,
                                kind: SymbolKind::METHOD,
                                tags: None,
                                detail: None,
                                uri: uri.clone(),
                                range: doc.text_range_to_lsp(method.range),
                                selection_range: doc.text_range_to_lsp(method.name.range),
                                data: None,
                            }]));
                        }
                    }
                    ("Class", SymbolKind::CLASS, cls.range, cls.name.range)
                }
                _ => continue,
            };
            if !contains(item_range, offset) {
                continue;
            }
            let item_name = match item {
                ast::TopLevelItem::Function(f) => f.name.name.clone(),
                ast::TopLevelItem::FunctionBlock(fb) => fb.name.name.clone(),
                ast::TopLevelItem::Program(p) => p.name.name.clone(),
                ast::TopLevelItem::Class(c) => c.name.name.clone(),
                _ => continue,
            };
            return Ok(Some(vec![CallHierarchyItem {
                name: item_name,
                kind,
                tags: None,
                detail: Some(name.to_string()),
                uri: uri.clone(),
                range: doc.text_range_to_lsp(item_range),
                selection_range: doc.text_range_to_lsp(name_range),
                data: None,
            }]));
        }
        Ok(None)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        let target_name = &params.item.name;
        let docs = self.documents.read().await;
        let mut results: Vec<CallHierarchyIncomingCall> = Vec::new();

        // Search ALL open documents for calls to the target function.
        for (uri, doc) in docs.iter() {
            for item in &doc.ast.items {
                let (caller_name, caller_kind, caller_range, caller_name_range, body) =
                    match item {
                        ast::TopLevelItem::Function(f) => (
                            f.name.name.clone(),
                            SymbolKind::FUNCTION,
                            f.range,
                            f.name.range,
                            f.body.as_slice(),
                        ),
                        ast::TopLevelItem::FunctionBlock(fb) => (
                            fb.name.name.clone(),
                            SymbolKind::CLASS,
                            fb.range,
                            fb.name.range,
                            fb.body.as_slice(),
                        ),
                        ast::TopLevelItem::Program(p) => (
                            p.name.name.clone(),
                            SymbolKind::MODULE,
                            p.range,
                            p.name.range,
                            p.body.as_slice(),
                        ),
                        ast::TopLevelItem::Class(cls) => {
                            // Check each method
                            for method in &cls.methods {
                                let mut ranges = Vec::new();
                                collect_call_ranges_in_stmts(
                                    &method.body,
                                    target_name,
                                    doc,
                                    &mut ranges,
                                );
                                if !ranges.is_empty() {
                                    results.push(CallHierarchyIncomingCall {
                                        from: CallHierarchyItem {
                                            name: format!(
                                                "{}.{}",
                                                cls.name.name, method.name.name
                                            ),
                                            kind: SymbolKind::METHOD,
                                            tags: None,
                                            detail: None,
                                            uri: uri.clone(),
                                            range: doc.text_range_to_lsp(method.range),
                                            selection_range: doc
                                                .text_range_to_lsp(method.name.range),
                                            data: None,
                                        },
                                        from_ranges: ranges,
                                    });
                                }
                            }
                            continue;
                        }
                        _ => continue,
                    };

                let mut ranges = Vec::new();
                collect_call_ranges_in_stmts(body, target_name, doc, &mut ranges);
                if !ranges.is_empty() {
                    results.push(CallHierarchyIncomingCall {
                        from: CallHierarchyItem {
                            name: caller_name,
                            kind: caller_kind,
                            tags: None,
                            detail: None,
                            uri: uri.clone(),
                            range: doc.text_range_to_lsp(caller_range),
                            selection_range: doc.text_range_to_lsp(caller_name_range),
                            data: None,
                        },
                        from_ranges: ranges,
                    });
                }
            }
        }

        if results.is_empty() {
            Ok(None)
        } else {
            Ok(Some(results))
        }
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        let source_name = &params.item.name;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(&params.item.uri) else {
            return Ok(None);
        };

        // Find the POU body for the source function and collect all calls it makes.
        let body = find_pou_body(&doc.ast, source_name);
        if body.is_empty() {
            return Ok(None);
        }

        // Collect all unique function calls and their ranges.
        let mut call_map: std::collections::HashMap<String, Vec<Range>> =
            std::collections::HashMap::new();
        collect_all_call_names_in_stmts(body, doc, &mut call_map);

        let mut results: Vec<CallHierarchyOutgoingCall> = Vec::new();
        for (callee_name, from_ranges) in call_map {
            // Resolve the callee to find its definition range.
            let global_scope = doc.analysis.symbols.global_scope_id();
            let (callee_kind, callee_range, callee_sel_range) =
                if let Some((_, sym)) = doc.analysis.symbols.resolve(global_scope, &callee_name) {
                    let kind = match &sym.kind {
                        st_semantics::scope::SymbolKind::Function { .. } => SymbolKind::FUNCTION,
                        st_semantics::scope::SymbolKind::FunctionBlock { .. } => SymbolKind::CLASS,
                        st_semantics::scope::SymbolKind::Program { .. } => SymbolKind::MODULE,
                        _ => SymbolKind::FUNCTION,
                    };
                    let lsp_range = doc.text_range_to_lsp(sym.range);
                    (kind, lsp_range, lsp_range)
                } else {
                    // Symbol not found — use a zero range
                    let zero = Range::default();
                    (SymbolKind::FUNCTION, zero, zero)
                };

            results.push(CallHierarchyOutgoingCall {
                to: CallHierarchyItem {
                    name: callee_name,
                    kind: callee_kind,
                    tags: None,
                    detail: None,
                    uri: params.item.uri.clone(),
                    range: callee_range,
                    selection_range: callee_sel_range,
                    data: None,
                },
                from_ranges,
            });
        }

        if results.is_empty() {
            Ok(None)
        } else {
            Ok(Some(results))
        }
    }

    // ── Go to Type Definition ───────────────────────────────────────
    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let offset = doc.position_to_offset(pos);
        if let Some((word, scope_id)) = self.resolve_at_position(doc, offset) {
            if let Some((_sid, sym)) = doc.analysis.symbols.resolve(scope_id, &word) {
                // Find the type name of this variable's type
                let type_name = match &sym.ty {
                    st_semantics::types::Ty::Struct { name, .. } => Some(name.clone()),
                    st_semantics::types::Ty::Enum { name, .. } => Some(name.clone()),
                    st_semantics::types::Ty::FunctionBlock { name } => Some(name.clone()),
                    st_semantics::types::Ty::Class { name } => Some(name.clone()),
                    st_semantics::types::Ty::Interface { name } => Some(name.clone()),
                    st_semantics::types::Ty::Subrange { name, .. } => Some(name.clone()),
                    st_semantics::types::Ty::Alias { name, .. } => Some(name.clone()),
                    _ => None,
                };

                if let Some(type_name) = type_name {
                    let global = doc.analysis.symbols.global_scope_id();
                    if let Some(type_sym) = doc.analysis.symbols.resolve(global, &type_name) {
                        let sym_range = type_sym.1.range;
                        // Try cross-file first (avoids false positives from offset overlap)
                        if !doc.project_files.is_empty() {
                            if let Some(location) = self.resolve_cross_file_location(doc, sym_range) {
                                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                            }
                        }
                        if sym_range.end <= doc.source.len() {
                            let range = doc.text_range_to_lsp(sym_range);
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: uri.clone(),
                                range,
                            })));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    // ── Workspace Symbol ────────────────────────────────────────────
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = params.query.to_uppercase();
        let docs = self.documents.read().await;
        let mut symbols = Vec::new();

        for (uri, doc) in docs.iter() {
            for item in &doc.ast.items {
                let (name, kind, range) = match item {
                    st_syntax::ast::TopLevelItem::Program(p) => {
                        (p.name.name.clone(), SymbolKind::MODULE, p.range)
                    }
                    st_syntax::ast::TopLevelItem::Function(f) => {
                        (f.name.name.clone(), SymbolKind::FUNCTION, f.range)
                    }
                    st_syntax::ast::TopLevelItem::FunctionBlock(fb) => {
                        (fb.name.name.clone(), SymbolKind::CLASS, fb.range)
                    }
                    st_syntax::ast::TopLevelItem::Class(cls) => {
                        (cls.name.name.clone(), SymbolKind::CLASS, cls.range)
                    }
                    st_syntax::ast::TopLevelItem::Interface(iface) => {
                        (iface.name.name.clone(), SymbolKind::INTERFACE, iface.range)
                    }
                    st_syntax::ast::TopLevelItem::TypeDeclaration(td) => {
                        if let Some(def) = td.definitions.first() {
                            (def.name.name.clone(), SymbolKind::STRUCT, def.range)
                        } else {
                            continue;
                        }
                    }
                    st_syntax::ast::TopLevelItem::GlobalVarDeclaration(_) => continue,
                };

                if query.is_empty() || name.to_uppercase().contains(&query) {
                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name,
                        kind,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: uri.clone(),
                            range: doc.text_range_to_lsp(range),
                        },
                        container_name: None,
                    });
                }
            }
        }

        if symbols.is_empty() { Ok(None) } else { Ok(Some(symbols)) }
    }

    // ── Document Links ──────────────────────────────────────────────
    async fn document_link(
        &self,
        params: DocumentLinkParams,
    ) -> Result<Option<Vec<DocumentLink>>> {
        let uri = &params.text_document.uri;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else { return Ok(None) };

        let mut links = Vec::new();

        // Find file paths in comments (simple heuristic: look for .st or .scl references)
        for (line_num, line) in doc.source.lines().enumerate() {
            let trimmed = line.trim();
            // Only look in comments
            if !(trimmed.starts_with("//") || trimmed.starts_with("(*") || trimmed.contains("(*")) {
                continue;
            }
            // Find patterns like filename.st or path/to/file.st
            for word in trimmed.split_whitespace() {
                let clean = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '/' && c != '_' && c != '-');
                if (clean.ends_with(".st") || clean.ends_with(".scl")) && clean.len() > 3 {
                    if let Some(col) = line.find(clean) {
                        // Try to resolve relative to the document
                        let uri_path: &str = uri.path();
                        if let Ok(base) = std::path::Path::new(uri_path).parent()
                            .ok_or("no parent")
                        {
                            let target = base.join(clean);
                            if let Ok(target_uri) = Url::from_file_path(&target) {
                                links.push(DocumentLink {
                                    range: Range::new(
                                        Position::new(line_num as u32, col as u32),
                                        Position::new(line_num as u32, (col + clean.len()) as u32),
                                    ),
                                    target: Some(target_uri),
                                    tooltip: Some(format!("Open {clean}")),
                                    data: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        if links.is_empty() { Ok(None) } else { Ok(Some(links)) }
    }
}

impl Backend {
    /// Get the word at the given byte offset.
    fn get_word_at(&self, doc: &Document, offset: usize) -> String {
        let bytes = doc.source.as_bytes();
        if offset >= bytes.len() {
            return String::new();
        }
        let mut start = offset;
        while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
            start -= 1;
        }
        let mut end = offset;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        if start == end { return String::new(); }
        std::str::from_utf8(&bytes[start..end]).unwrap_or("").to_string()
    }
}

/// Simple ST formatter: normalizes indentation.
fn format_st_source(source: &str, tab_size: usize) -> String {
    let indent_str = " ".repeat(tab_size);
    let mut result = String::new();
    let mut indent_level: i32 = 0;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            result.push('\n');
            continue;
        }

        let upper = trimmed.to_uppercase();

        // Decrease indent for closing keywords
        if upper.starts_with("END_")
            || upper.starts_with("END_VAR")
            || upper == "ELSE"
            || upper.starts_with("ELSIF")
            || upper.starts_with("UNTIL")
        {
            indent_level = (indent_level - 1).max(0);
        }

        // Write indented line
        for _ in 0..indent_level {
            result.push_str(&indent_str);
        }
        result.push_str(trimmed);
        result.push('\n');

        // Increase indent for opening keywords
        if upper.starts_with("PROGRAM ")
            || upper.starts_with("FUNCTION ")
            || upper.starts_with("FUNCTION_BLOCK ")
            || upper.starts_with("VAR")
            || upper.starts_with("IF ") || upper == "ELSE"
            || upper.starts_with("ELSIF ")
            || upper.starts_with("FOR ")
            || upper.starts_with("WHILE ")
            || upper.starts_with("REPEAT")
            || upper.starts_with("CASE ")
            || upper.starts_with("STRUCT")
            || upper.starts_with("TYPE")
        {
            indent_level += 1;
        }
    }

    result
}

/// Find the byte offset where a new variable declaration can be inserted
/// (right after the last variable in the first VAR block).
fn find_var_block_insert_position(source: &str) -> Option<usize> {
    let upper = source.to_uppercase();
    // Find first END_VAR
    let end_var_pos = upper.find("END_VAR")?;
    // Insert just before END_VAR
    // Find the start of the END_VAR line
    let line_start = source[..end_var_pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    Some(line_start)
}

#[allow(deprecated)]
fn add_var_symbols(
    doc: &Document,
    var_blocks: &[st_syntax::ast::VarBlock],
    symbols: &mut Vec<DocumentSymbol>,
) {
    for vb in var_blocks {
        for decl in &vb.declarations {
            for name in &decl.names {
                symbols.push(DocumentSymbol {
                    name: name.name.clone(),
                    detail: Some(format!("{:?} : {}", vb.kind, type_display(&decl.ty))),
                    kind: SymbolKind::VARIABLE,
                    range: doc.text_range_to_lsp(decl.range),
                    selection_range: doc.text_range_to_lsp(name.range),
                    children: None,
                    tags: None,
                    deprecated: None,
                });
            }
        }
    }
}

fn type_display(dt: &st_syntax::ast::DataType) -> String {
    match dt {
        st_syntax::ast::DataType::Elementary(e) => {
            st_semantics::types::elementary_name(*e).to_string()
        }
        st_syntax::ast::DataType::Array(arr) => {
            format!("ARRAY OF {}", type_display(&arr.element_type))
        }
        st_syntax::ast::DataType::String(s) => {
            if s.wide { "WSTRING".to_string() } else { "STRING".to_string() }
        }
        st_syntax::ast::DataType::Ref(inner) => {
            format!("REF_TO {}", type_display(inner))
        }
        st_syntax::ast::DataType::UserDefined(qn) => qn.as_str(),
    }
}

// =============================================================================
// Selection Range helpers
// =============================================================================

use st_syntax::ast::{
    self as ast, Argument, ElsifClause, ForStmt, IfStmt, RepeatStmt, SourceFile,
    WhileStmt,
};

// =============================================================================
// Call Hierarchy helpers
// =============================================================================

/// Find all call sites to `target_name` within a statement list.
/// Returns the LSP ranges of each call expression.
fn collect_call_ranges_in_stmts(
    stmts: &[ast::Statement],
    target_name: &str,
    doc: &crate::document::Document,
    ranges: &mut Vec<Range>,
) {
    for stmt in stmts {
        match stmt {
            ast::Statement::FunctionCall(fc) => {
                if fc.name.as_str().eq_ignore_ascii_case(target_name) {
                    ranges.push(doc.text_range_to_lsp(fc.range));
                }
            }
            ast::Statement::Assignment(a) => {
                collect_call_ranges_in_expr(&a.value, target_name, doc, ranges);
            }
            ast::Statement::If(IfStmt {
                condition,
                then_body,
                elsif_clauses,
                else_body,
                ..
            }) => {
                collect_call_ranges_in_expr(condition, target_name, doc, ranges);
                collect_call_ranges_in_stmts(then_body, target_name, doc, ranges);
                for clause in elsif_clauses {
                    collect_call_ranges_in_expr(&clause.condition, target_name, doc, ranges);
                    collect_call_ranges_in_stmts(&clause.body, target_name, doc, ranges);
                }
                if let Some(els) = else_body {
                    collect_call_ranges_in_stmts(els, target_name, doc, ranges);
                }
            }
            ast::Statement::For(ForStmt { body, .. }) => {
                collect_call_ranges_in_stmts(body, target_name, doc, ranges);
            }
            ast::Statement::While(WhileStmt {
                condition, body, ..
            }) => {
                collect_call_ranges_in_expr(condition, target_name, doc, ranges);
                collect_call_ranges_in_stmts(body, target_name, doc, ranges);
            }
            ast::Statement::Repeat(RepeatStmt {
                body, condition, ..
            }) => {
                collect_call_ranges_in_stmts(body, target_name, doc, ranges);
                collect_call_ranges_in_expr(condition, target_name, doc, ranges);
            }
            ast::Statement::Case(ast::CaseStmt {
                branches,
                else_body,
                ..
            }) => {
                for branch in branches {
                    collect_call_ranges_in_stmts(&branch.body, target_name, doc, ranges);
                }
                if let Some(els) = else_body {
                    collect_call_ranges_in_stmts(els, target_name, doc, ranges);
                }
            }
            _ => {}
        }
    }
}

fn collect_call_ranges_in_expr(
    expr: &ast::Expression,
    target_name: &str,
    doc: &crate::document::Document,
    ranges: &mut Vec<Range>,
) {
    match expr {
        ast::Expression::FunctionCall(fc) => {
            if fc.name.as_str().eq_ignore_ascii_case(target_name) {
                ranges.push(doc.text_range_to_lsp(fc.range));
            }
            // Also check arguments for nested calls
            for arg in &fc.arguments {
                match arg {
                    Argument::Positional(e) => {
                        collect_call_ranges_in_expr(e, target_name, doc, ranges);
                    }
                    Argument::Named { value, .. } => {
                        collect_call_ranges_in_expr(value, target_name, doc, ranges);
                    }
                }
            }
        }
        ast::Expression::Binary(b) => {
            collect_call_ranges_in_expr(&b.left, target_name, doc, ranges);
            collect_call_ranges_in_expr(&b.right, target_name, doc, ranges);
        }
        ast::Expression::Unary(u) => {
            collect_call_ranges_in_expr(&u.operand, target_name, doc, ranges);
        }
        ast::Expression::Parenthesized(inner) => {
            collect_call_ranges_in_expr(inner, target_name, doc, ranges);
        }
        _ => {}
    }
}

/// Collect ALL unique function calls made within a statement list.
/// Builds a map from callee name → list of call-site ranges.
fn collect_all_call_names_in_stmts(
    stmts: &[ast::Statement],
    doc: &crate::document::Document,
    call_map: &mut std::collections::HashMap<String, Vec<Range>>,
) {
    for stmt in stmts {
        match stmt {
            ast::Statement::FunctionCall(fc) => {
                let name = fc.name.as_str();
                call_map
                    .entry(name.to_uppercase())
                    .or_default()
                    .push(doc.text_range_to_lsp(fc.range));
            }
            ast::Statement::Assignment(a) => {
                collect_all_call_names_in_expr(&a.value, doc, call_map);
            }
            ast::Statement::If(IfStmt {
                condition,
                then_body,
                elsif_clauses,
                else_body,
                ..
            }) => {
                collect_all_call_names_in_expr(condition, doc, call_map);
                collect_all_call_names_in_stmts(then_body, doc, call_map);
                for clause in elsif_clauses {
                    collect_all_call_names_in_expr(&clause.condition, doc, call_map);
                    collect_all_call_names_in_stmts(&clause.body, doc, call_map);
                }
                if let Some(els) = else_body {
                    collect_all_call_names_in_stmts(els, doc, call_map);
                }
            }
            ast::Statement::For(ForStmt { body, .. }) => {
                collect_all_call_names_in_stmts(body, doc, call_map);
            }
            ast::Statement::While(WhileStmt {
                condition, body, ..
            }) => {
                collect_all_call_names_in_expr(condition, doc, call_map);
                collect_all_call_names_in_stmts(body, doc, call_map);
            }
            ast::Statement::Repeat(RepeatStmt {
                body, condition, ..
            }) => {
                collect_all_call_names_in_stmts(body, doc, call_map);
                collect_all_call_names_in_expr(condition, doc, call_map);
            }
            ast::Statement::Case(ast::CaseStmt {
                branches,
                else_body,
                ..
            }) => {
                for branch in branches {
                    collect_all_call_names_in_stmts(&branch.body, doc, call_map);
                }
                if let Some(els) = else_body {
                    collect_all_call_names_in_stmts(els, doc, call_map);
                }
            }
            _ => {}
        }
    }
}

fn collect_all_call_names_in_expr(
    expr: &ast::Expression,
    doc: &crate::document::Document,
    call_map: &mut std::collections::HashMap<String, Vec<Range>>,
) {
    match expr {
        ast::Expression::FunctionCall(fc) => {
            let name = fc.name.as_str();
            call_map
                .entry(name.to_uppercase())
                .or_default()
                .push(doc.text_range_to_lsp(fc.range));
            for arg in &fc.arguments {
                match arg {
                    Argument::Positional(e) => {
                        collect_all_call_names_in_expr(e, doc, call_map);
                    }
                    Argument::Named { value, .. } => {
                        collect_all_call_names_in_expr(value, doc, call_map);
                    }
                }
            }
        }
        ast::Expression::Binary(b) => {
            collect_all_call_names_in_expr(&b.left, doc, call_map);
            collect_all_call_names_in_expr(&b.right, doc, call_map);
        }
        ast::Expression::Unary(u) => {
            collect_all_call_names_in_expr(&u.operand, doc, call_map);
        }
        ast::Expression::Parenthesized(inner) => {
            collect_all_call_names_in_expr(inner, doc, call_map);
        }
        _ => {}
    }
}

/// Find the body statements for a POU by name (case-insensitive).
/// For class methods, the name is "ClassName.MethodName".
fn find_pou_body<'a>(ast: &'a SourceFile, name: &str) -> &'a [ast::Statement] {
    for item in &ast.items {
        match item {
            ast::TopLevelItem::Function(f)
                if f.name.name.eq_ignore_ascii_case(name) =>
            {
                return &f.body;
            }
            ast::TopLevelItem::FunctionBlock(fb)
                if fb.name.name.eq_ignore_ascii_case(name) =>
            {
                return &fb.body;
            }
            ast::TopLevelItem::Program(p)
                if p.name.name.eq_ignore_ascii_case(name) =>
            {
                return &p.body;
            }
            ast::TopLevelItem::Class(cls) => {
                for method in &cls.methods {
                    let full = format!("{}.{}", cls.name.name, method.name.name);
                    if full.eq_ignore_ascii_case(name) {
                        return &method.body;
                    }
                }
            }
            _ => {}
        }
    }
    &[]
}

// =============================================================================
// Inlay Hint helpers
// =============================================================================

/// Walk a statement list looking for FunctionCallExpr nodes and emit
/// parameter-name hints for positional arguments.
fn collect_call_hints(
    stmts: &[ast::Statement],
    doc: &crate::document::Document,
    range_start: usize,
    range_end: usize,
    hints: &mut Vec<InlayHint>,
) {
    for stmt in stmts {
        let sr = stmt.range();
        if sr.end < range_start || sr.start > range_end {
            continue;
        }
        match stmt {
            ast::Statement::FunctionCall(fc) => {
                emit_call_hints(fc, doc, hints);
            }
            ast::Statement::Assignment(a) => {
                // The RHS might be a function call: `result := Helper(10, 20);`
                collect_expr_call_hints(&a.value, doc, hints);
            }
            ast::Statement::If(IfStmt {
                condition,
                then_body,
                elsif_clauses,
                else_body,
                ..
            }) => {
                collect_expr_call_hints(condition, doc, hints);
                collect_call_hints(then_body, doc, range_start, range_end, hints);
                for clause in elsif_clauses {
                    collect_expr_call_hints(&clause.condition, doc, hints);
                    collect_call_hints(&clause.body, doc, range_start, range_end, hints);
                }
                if let Some(els) = else_body {
                    collect_call_hints(els, doc, range_start, range_end, hints);
                }
            }
            ast::Statement::For(ForStmt { body, .. }) => {
                collect_call_hints(body, doc, range_start, range_end, hints);
            }
            ast::Statement::While(WhileStmt {
                condition, body, ..
            }) => {
                collect_expr_call_hints(condition, doc, hints);
                collect_call_hints(body, doc, range_start, range_end, hints);
            }
            ast::Statement::Repeat(RepeatStmt {
                body, condition, ..
            }) => {
                collect_call_hints(body, doc, range_start, range_end, hints);
                collect_expr_call_hints(condition, doc, hints);
            }
            ast::Statement::Case(ast::CaseStmt {
                branches,
                else_body,
                ..
            }) => {
                for branch in branches {
                    collect_call_hints(&branch.body, doc, range_start, range_end, hints);
                }
                if let Some(els) = else_body {
                    collect_call_hints(els, doc, range_start, range_end, hints);
                }
            }
            _ => {}
        }
    }
}

/// Check if an expression contains a function call and emit hints for it.
fn collect_expr_call_hints(
    expr: &ast::Expression,
    doc: &crate::document::Document,
    hints: &mut Vec<InlayHint>,
) {
    match expr {
        ast::Expression::FunctionCall(fc) => {
            emit_call_hints(fc, doc, hints);
        }
        ast::Expression::Binary(b) => {
            collect_expr_call_hints(&b.left, doc, hints);
            collect_expr_call_hints(&b.right, doc, hints);
        }
        ast::Expression::Unary(u) => {
            collect_expr_call_hints(&u.operand, doc, hints);
        }
        ast::Expression::Parenthesized(inner) => {
            collect_expr_call_hints(inner, doc, hints);
        }
        _ => {}
    }
}

/// For a single function/FB call, resolve the callee's parameter list and
/// emit `paramName:` hints before each positional argument.
fn emit_call_hints(
    fc: &ast::FunctionCallExpr,
    doc: &crate::document::Document,
    hints: &mut Vec<InlayHint>,
) {
    // Only generate hints if there are positional arguments — named
    // arguments already show the parameter name explicitly.
    let has_positional = fc
        .arguments
        .iter()
        .any(|a| matches!(a, Argument::Positional(_)));
    if !has_positional {
        return;
    }

    // Resolve the function name in the symbol table.
    let func_name = fc.name.as_str();
    let global_scope = doc.analysis.symbols.global_scope_id();
    let params = match doc.analysis.symbols.resolve(global_scope, &func_name) {
        Some((_, sym)) => match &sym.kind {
            st_semantics::scope::SymbolKind::Function { params, .. } => params.clone(),
            st_semantics::scope::SymbolKind::FunctionBlock { params, .. } => params.clone(),
            st_semantics::scope::SymbolKind::Program { params, .. } => params.clone(),
            _ => return,
        },
        None => return,
    };

    // Match positional arguments to parameters by index.
    let mut param_idx = 0;
    for arg in &fc.arguments {
        let Argument::Positional(expr) = arg else {
            // Named arguments use the given name; skip incrementing the
            // positional index.
            continue;
        };
        if param_idx >= params.len() {
            break;
        }
        let param = &params[param_idx];
        param_idx += 1;

        // Don't show a hint if the argument text matches the parameter
        // name (e.g., `Helper(x)` where the parameter is also `x`).
        let arg_text = &doc.source[expr.range().start..expr.range().end];
        if arg_text.trim().eq_ignore_ascii_case(&param.name) {
            continue;
        }

        hints.push(InlayHint {
            position: doc.offset_to_position(expr.range().start),
            label: InlayHintLabel::String(format!("{}:", param.name)),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: Some(InlayHintTooltip::String(format!(
                "{}: {}",
                param.name,
                param.ty.display_name()
            ))),
            padding_left: None,
            padding_right: Some(true),
            data: None,
        });
    }
}

/// Collect every AST node range that contains `offset`, sorted from
/// outermost (SourceFile) to innermost (deepest nested statement/expression).
fn collect_containing_ranges(source_file: &SourceFile, offset: usize) -> Vec<ast::TextRange> {
    let mut ranges = Vec::new();

    // Level 0: whole file
    if contains(source_file.range, offset) {
        ranges.push(source_file.range);
    }

    for item in &source_file.items {
        let item_range = top_level_range(item);
        if !contains(item_range, offset) {
            continue;
        }
        ranges.push(item_range);

        // Recurse into the item's children (var blocks, body statements, methods)
        match item {
            ast::TopLevelItem::Program(p) => {
                collect_var_blocks(&p.var_blocks, offset, &mut ranges);
                collect_statements(&p.body, offset, &mut ranges);
            }
            ast::TopLevelItem::Function(f) => {
                collect_var_blocks(&f.var_blocks, offset, &mut ranges);
                collect_statements(&f.body, offset, &mut ranges);
            }
            ast::TopLevelItem::FunctionBlock(fb) => {
                collect_var_blocks(&fb.var_blocks, offset, &mut ranges);
                collect_statements(&fb.body, offset, &mut ranges);
            }
            ast::TopLevelItem::Class(cls) => {
                collect_var_blocks(&cls.var_blocks, offset, &mut ranges);
                for method in &cls.methods {
                    if contains(method.range, offset) {
                        ranges.push(method.range);
                        collect_var_blocks(&method.var_blocks, offset, &mut ranges);
                        collect_statements(&method.body, offset, &mut ranges);
                    }
                }
            }
            ast::TopLevelItem::Interface(iface) => {
                if contains(iface.range, offset) {
                    ranges.push(iface.range);
                }
            }
            ast::TopLevelItem::TypeDeclaration(td) => {
                if contains(td.range, offset) {
                    ranges.push(td.range);
                }
            }
            ast::TopLevelItem::GlobalVarDeclaration(vb) => {
                // The item_range already covers the whole VarBlock.
                collect_var_decls(&vb.declarations, offset, &mut ranges);
            }
        }
    }

    ranges
}

fn collect_var_blocks(
    blocks: &[ast::VarBlock],
    offset: usize,
    ranges: &mut Vec<ast::TextRange>,
) {
    for vb in blocks {
        if contains(vb.range, offset) {
            ranges.push(vb.range);
            collect_var_decls(&vb.declarations, offset, ranges);
        }
    }
}

fn collect_var_decls(
    decls: &[ast::VarDeclaration],
    offset: usize,
    ranges: &mut Vec<ast::TextRange>,
) {
    for decl in decls {
        if contains(decl.range, offset) {
            ranges.push(decl.range);
        }
    }
}

fn collect_statements(
    stmts: &[ast::Statement],
    offset: usize,
    ranges: &mut Vec<ast::TextRange>,
) {
    for stmt in stmts {
        let r = stmt.range();
        if !contains(r, offset) {
            continue;
        }
        ranges.push(r);

        // Recurse into compound statements
        match stmt {
            ast::Statement::If(IfStmt {
                then_body,
                elsif_clauses,
                else_body,
                ..
            }) => {
                collect_statements(then_body, offset, ranges);
                for ElsifClause { body, range, .. } in elsif_clauses {
                    if contains(*range, offset) {
                        ranges.push(*range);
                        collect_statements(body, offset, ranges);
                    }
                }
                if let Some(els) = else_body {
                    collect_statements(els, offset, ranges);
                }
            }
            ast::Statement::For(ForStmt { body, .. }) => {
                collect_statements(body, offset, ranges);
            }
            ast::Statement::While(WhileStmt { body, .. }) => {
                collect_statements(body, offset, ranges);
            }
            ast::Statement::Repeat(RepeatStmt { body, .. }) => {
                collect_statements(body, offset, ranges);
            }
            ast::Statement::Case(ast::CaseStmt {
                branches,
                else_body,
                ..
            }) => {
                for branch in branches {
                    if contains(branch.range, offset) {
                        ranges.push(branch.range);
                        collect_statements(&branch.body, offset, ranges);
                    }
                }
                if let Some(els) = else_body {
                    collect_statements(els, offset, ranges);
                }
            }
            _ => {}
        }
    }
}

fn top_level_range(item: &ast::TopLevelItem) -> ast::TextRange {
    match item {
        ast::TopLevelItem::Program(p) => p.range,
        ast::TopLevelItem::Function(f) => f.range,
        ast::TopLevelItem::FunctionBlock(fb) => fb.range,
        ast::TopLevelItem::Class(c) => c.range,
        ast::TopLevelItem::Interface(i) => i.range,
        ast::TopLevelItem::TypeDeclaration(t) => t.range,
        ast::TopLevelItem::GlobalVarDeclaration(v) => v.range,
    }
}

fn contains(range: ast::TextRange, offset: usize) -> bool {
    offset >= range.start && offset <= range.end
}

/// Find the contiguous word (identifier or number) surrounding `offset`.
/// Returns the byte range of the word, or None if the cursor is on whitespace
/// or a punctuation character.
fn find_word_range(source: &str, offset: usize) -> Option<ast::TextRange> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() {
        return None;
    }
    let at = bytes[offset];
    if !at.is_ascii_alphanumeric() && at != b'_' {
        return None;
    }
    let mut start = offset;
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_')
    {
        start -= 1;
    }
    let mut end = offset;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    Some(ast::TextRange::new(start, end))
}

// =============================================================================
// On-type formatting helpers
// =============================================================================

/// Extract the leading whitespace of a line.
fn leading_whitespace(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}

// =============================================================================
// Linked Editing Range helpers
// =============================================================================

/// The set of IEC 61131-3 keyword pairs we support for linked editing.
/// Each entry: (opening keyword, closing keyword).
const KEYWORD_PAIRS: &[(&str, &str)] = &[
    ("IF", "END_IF"),
    ("FOR", "END_FOR"),
    ("WHILE", "END_WHILE"),
    ("REPEAT", "END_REPEAT"),
    ("CASE", "END_CASE"),
    ("PROGRAM", "END_PROGRAM"),
    ("FUNCTION", "END_FUNCTION"),
    ("FUNCTION_BLOCK", "END_FUNCTION_BLOCK"),
    ("VAR", "END_VAR"),
    ("VAR_INPUT", "END_VAR"),
    ("VAR_OUTPUT", "END_VAR"),
    ("VAR_IN_OUT", "END_VAR"),
    ("VAR_GLOBAL", "END_VAR"),
    ("VAR_EXTERNAL", "END_VAR"),
    ("CLASS", "END_CLASS"),
    ("METHOD", "END_METHOD"),
    ("INTERFACE", "END_INTERFACE"),
    ("TYPE", "END_TYPE"),
    ("STRUCT", "END_STRUCT"),
];

/// Find the opening and closing keyword ranges for a block, given the
/// cursor is on `word` (uppercased) at `offset`. Uses the AST to scope the
/// search to the correct nesting level, then extracts keyword byte ranges
/// from the source text.
fn find_keyword_pair(
    ast: &SourceFile,
    source: &str,
    offset: usize,
    word: &str,
) -> Option<(ast::TextRange, ast::TextRange)> {
    // Determine which keyword pair this word belongs to.
    let (open_kw, close_kw) = KEYWORD_PAIRS
        .iter()
        .find(|(o, c)| *o == word || *c == word)
        .copied()?;

    // Walk the AST to find the innermost block of the right type that
    // contains the cursor offset.
    let block_range = find_innermost_block_range(ast, source, offset, open_kw, close_kw)?;

    // Extract the opening keyword position: it's at the very start of the
    // block's range (possibly after indentation whitespace within the
    // block's byte range).
    let block_src = &source[block_range.start..block_range.end];
    let trimmed_start = block_src.len() - block_src.trim_start().len();
    let open_start = block_range.start + trimmed_start;
    let open_end = open_start + open_kw.len();

    // Verify the source actually has the expected opening keyword.
    let open_text = source
        .get(open_start..open_end)
        .unwrap_or("")
        .to_uppercase();
    if open_text != open_kw {
        return None;
    }

    // Extract the closing keyword position: search backward from the
    // block's end, skipping `;`, whitespace, and looking for the keyword.
    let close_start = source[..block_range.end]
        .to_uppercase()
        .rfind(close_kw)?;
    let close_end = close_start + close_kw.len();

    // Sanity: closing must be inside the block range and after the opening.
    if close_start < open_end || close_end > block_range.end {
        return None;
    }

    Some((
        ast::TextRange::new(open_start, open_end),
        ast::TextRange::new(close_start, close_end),
    ))
}

/// Walk the AST to find the innermost block whose open/close keywords
/// match `open_kw`/`close_kw` and whose range contains `offset`.
fn find_innermost_block_range(
    ast: &SourceFile,
    source: &str,
    offset: usize,
    open_kw: &str,
    close_kw: &str,
) -> Option<ast::TextRange> {
    let mut best: Option<ast::TextRange> = None;

    // Check top-level items
    for item in &ast.items {
        let item_range = top_level_range(item);
        if !contains(item_range, offset) {
            continue;
        }

        // Does this item's keyword match?
        if keyword_at_range_start(source, item_range, open_kw) {
            best = narrower(best, item_range);
        }

        // Check VAR blocks inside POUs
        let var_blocks: &[ast::VarBlock] = match item {
            ast::TopLevelItem::Program(p) => &p.var_blocks,
            ast::TopLevelItem::Function(f) => &f.var_blocks,
            ast::TopLevelItem::FunctionBlock(fb) => &fb.var_blocks,
            ast::TopLevelItem::Class(cls) => &cls.var_blocks,
            _ => &[],
        };
        for vb in var_blocks {
            if contains(vb.range, offset) && keyword_at_range_start(source, vb.range, open_kw) {
                best = narrower(best, vb.range);
            }
        }

        // Check statements recursively
        let bodies: Vec<&[ast::Statement]> = match item {
            ast::TopLevelItem::Program(p) => vec![&p.body],
            ast::TopLevelItem::Function(f) => vec![&f.body],
            ast::TopLevelItem::FunctionBlock(fb) => vec![&fb.body],
            ast::TopLevelItem::Class(cls) => {
                cls.methods.iter().map(|m| m.body.as_slice()).collect()
            }
            _ => vec![],
        };
        for body in bodies {
            if let Some(r) = find_stmt_block(body, source, offset, open_kw, close_kw) {
                best = narrower(best, r);
            }
        }
    }

    best
}

fn find_stmt_block(
    stmts: &[ast::Statement],
    source: &str,
    offset: usize,
    open_kw: &str,
    _close_kw: &str,
) -> Option<ast::TextRange> {
    let mut best: Option<ast::TextRange> = None;
    for stmt in stmts {
        let r = stmt.range();
        if !contains(r, offset) {
            continue;
        }
        if keyword_at_range_start(source, r, open_kw) {
            best = narrower(best, r);
        }
        // Recurse into compound statements
        let sub_bodies: Vec<&[ast::Statement]> = match stmt {
            ast::Statement::If(s) => {
                let mut v: Vec<&[ast::Statement]> = vec![&s.then_body];
                for clause in &s.elsif_clauses {
                    v.push(&clause.body);
                }
                if let Some(els) = &s.else_body {
                    v.push(els);
                }
                v
            }
            ast::Statement::For(s) => vec![&s.body],
            ast::Statement::While(s) => vec![&s.body],
            ast::Statement::Repeat(s) => vec![&s.body],
            ast::Statement::Case(s) => {
                let mut v: Vec<&[ast::Statement]> = Vec::new();
                for branch in &s.branches {
                    v.push(&branch.body);
                }
                if let Some(els) = &s.else_body {
                    v.push(els);
                }
                v
            }
            _ => vec![],
        };
        for body in sub_bodies {
            if let Some(r) = find_stmt_block(body, source, offset, open_kw, _close_kw) {
                best = narrower(best, r);
            }
        }
    }
    best
}

/// Check if the source text at the start of `range` (after trimming
/// leading whitespace) begins with `keyword` (case-insensitive).
fn keyword_at_range_start(source: &str, range: ast::TextRange, keyword: &str) -> bool {
    let slice = &source[range.start..range.end.min(source.len())];
    let trimmed = slice.trim_start();
    trimmed
        .get(..keyword.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(keyword))
}

/// Return the narrower of two optional ranges (smaller span wins —
/// represents the innermost nesting level).
fn narrower(a: Option<ast::TextRange>, b: ast::TextRange) -> Option<ast::TextRange> {
    match a {
        Some(existing) if (existing.end - existing.start) <= (b.end - b.start) => Some(existing),
        _ => Some(b),
    }
}

/// True if the (uppercased, trimmed) line starts with a keyword that should
/// increase the indent of the NEXT line. Covers IEC 61131-3 block openers.
fn starts_with_opener(upper: &str) -> bool {
    // POU declarations
    upper.starts_with("PROGRAM ")
        || upper.starts_with("FUNCTION ")
        || upper.starts_with("FUNCTION_BLOCK ")
        || upper.starts_with("CLASS ")
        || upper.starts_with("METHOD ")
        || upper.starts_with("INTERFACE ")
        // Variable blocks
        || upper.starts_with("VAR")
            && (upper.len() == 3
                || upper.as_bytes().get(3).is_some_and(|b| !b.is_ascii_alphanumeric()))
        // Control flow
        || upper.ends_with("THEN")
        || upper.ends_with("DO")
            && (upper.starts_with("FOR ") || upper.starts_with("WHILE "))
        || upper.starts_with("REPEAT")
        || upper.starts_with("ELSE")
            && (upper.len() == 4
                || upper.as_bytes().get(4).is_some_and(|b| !b.is_ascii_alphanumeric()))
        || upper.ends_with("OF") && upper.starts_with("CASE ")
        // Type declarations
        || upper.starts_with("STRUCT")
        || upper.starts_with("TYPE")
            && (upper.len() == 4
                || upper.as_bytes().get(4).is_some_and(|b| !b.is_ascii_alphanumeric()))
}
