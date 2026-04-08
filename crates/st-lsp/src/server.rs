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

        // Parse / lower errors
        for err in &doc.lower_errors {
            diags.push(Diagnostic {
                range: doc.text_range_to_lsp(err.range),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("st".to_string()),
                message: err.message.clone(),
                ..Default::default()
            });
        }

        // Semantic diagnostics
        for d in &doc.analysis.diagnostics {
            let severity = match d.severity {
                st_semantics::diagnostic::Severity::Error => DiagnosticSeverity::ERROR,
                st_semantics::diagnostic::Severity::Warning => DiagnosticSeverity::WARNING,
                st_semantics::diagnostic::Severity::Info => DiagnosticSeverity::INFORMATION,
            };
            diags.push(Diagnostic {
                range: doc.text_range_to_lsp(d.range),
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

    /// Resolve a symbol's byte range to a Location in a cross-file project.
    /// Searches project_files to find which file the byte range belongs to.
    /// Uses the same approach as the DAP: parse each project file individually
    /// and check which one defines the symbol at the given byte range.
    fn resolve_cross_file_location(
        &self,
        doc: &Document,
        sym_range: st_syntax::ast::TextRange,
    ) -> Option<Location> {
        for (path, content) in &doc.project_files {
            if sym_range.end > content.len() || sym_range.start >= content.len() {
                continue;
            }
            // Verify: parse this file and check if any top-level item starts at sym_range.start
            let parse = st_syntax::parse(content);
            let has_item_at_range = parse.source_file.items.iter().any(|item| {
                let item_range = match item {
                    st_syntax::ast::TopLevelItem::Program(p) => p.range,
                    st_syntax::ast::TopLevelItem::Function(f) => f.range,
                    st_syntax::ast::TopLevelItem::FunctionBlock(fb) => fb.range,
                    st_syntax::ast::TopLevelItem::Class(cls) => cls.range,
                    st_syntax::ast::TopLevelItem::Interface(iface) => iface.range,
                    st_syntax::ast::TopLevelItem::TypeDeclaration(td) => td.range,
                    st_syntax::ast::TopLevelItem::GlobalVarDeclaration(vb) => vb.range,
                };
                // Check if the symbol range overlaps with this item
                sym_range.start >= item_range.start && sym_range.end <= item_range.end
            });
            if !has_item_at_range {
                continue;
            }

            let file_uri = tower_lsp::lsp_types::Url::from_file_path(path).ok()?;
            let src = content.as_bytes();
            let start_offset = sym_range.start.min(src.len());
            let end_offset = sym_range.end.min(src.len());
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

    /// Find the innermost scope containing the given offset.
    fn find_scope_for_offset(
        &self,
        doc: &Document,
        offset: usize,
    ) -> st_semantics::scope::ScopeId {
        let scopes = doc.analysis.symbols.scopes();
        // Walk scopes in reverse (deepest first) to find the innermost one
        // that contains the offset. We check POU names to find the right scope.
        let global = doc.analysis.symbols.global_scope_id();

        // Find the POU that contains this offset
        for item in &doc.ast.items {
            let (range, name) = match item {
                st_syntax::ast::TopLevelItem::Program(p) => (p.range, &p.name.name),
                st_syntax::ast::TopLevelItem::Function(f) => (f.range, &f.name.name),
                st_syntax::ast::TopLevelItem::FunctionBlock(fb) => (fb.range, &fb.name.name),
                _ => continue,
            };
            if range.start <= offset && offset <= range.end {
                // Find the scope for this POU
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
                doc.update(change.text, Some(params.text_document.version));
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

        if let Some((word, scope_id)) = self.resolve_at_position(doc, offset) {
            if let Some((_sid, sym)) =
                doc.analysis.symbols.resolve(scope_id, &word)
            {
                let sym_range = sym.range;

                // Check if the symbol's byte range falls within the current file
                if sym_range.end <= doc.source.len() {
                    let range = doc.text_range_to_lsp(sym_range);
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: uri.clone(),
                        range,
                    })));
                }

                // Symbol is in a different project file — find which one
                if let Some(location) = self.resolve_cross_file_location(doc, sym_range) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
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
                        if sym_range.end <= doc.source.len() {
                            let range = doc.text_range_to_lsp(sym_range);
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: uri.clone(),
                                range,
                            })));
                        }
                        if let Some(location) = self.resolve_cross_file_location(doc, sym_range) {
                            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
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
