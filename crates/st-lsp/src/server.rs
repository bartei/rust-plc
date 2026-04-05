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
        let doc = Document::new(
            params.text_document.text,
            Some(params.text_document.version),
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
                let doc = Document::new(change.text, Some(params.text_document.version));
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
                let range = doc.text_range_to_lsp(sym.range);
                return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range,
                })));
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
        st_syntax::ast::DataType::UserDefined(qn) => qn.as_str(),
    }
}
