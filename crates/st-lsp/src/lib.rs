//! Language Server Protocol implementation for Structured Text.
//!
//! Provides diagnostics, hover, go-to-definition, and semantic tokens
//! backed by incremental tree-sitter parsing and semantic analysis.

pub mod completion;
pub mod document;
pub mod semantic_tokens;
pub mod server;

use tower_lsp::{LspService, Server};

/// Run the LSP server on stdin/stdout.
pub async fn run_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
