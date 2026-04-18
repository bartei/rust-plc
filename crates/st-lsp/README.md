# st-lsp

Language Server Protocol implementation for IEC 61131-3 Structured Text.

## Purpose

Provides IDE support for Structured Text through the Language Server Protocol. Editors that support LSP (VS Code, Vim/Neovim, Sublime Text, Emacs) get diagnostics, code completion, go-to-definition, hover information, semantic highlighting, and more — all powered by the same parser and semantic analyzer used by the compiler.

## Public API

```rust
use st_lsp::run_stdio;

// Start the LSP server on stdin/stdout
run_stdio().await;
```

- `run_stdio()` — Starts the LSP server communicating over stdin/stdout (the standard mode for editor integration)

## Functional Description

### Supported LSP Features

| Feature | Description |
|---------|-------------|
| `textDocument/publishDiagnostics` | Real-time error and warning highlighting |
| `textDocument/completion` | Context-aware code completion (variables, functions, keywords) |
| `textDocument/hover` | Type information and documentation on hover |
| `textDocument/definition` | Go-to-definition (cross-file) |
| `textDocument/references` | Find all references |
| `textDocument/rename` | Safe rename across files |
| `textDocument/formatting` | Source code formatting |
| `textDocument/signatureHelp` | Function parameter hints |
| `textDocument/semanticTokens` | Semantic syntax highlighting |
| `textDocument/selectionRange` | Smart expand/shrink selection |
| `textDocument/inlayHint` | Parameter name hints at call sites |
| `textDocument/onTypeFormatting` | Auto-indent after Enter |
| `textDocument/callHierarchy` | Incoming/outgoing call graph |
| `textDocument/linkedEditingRange` | Matching keyword pair highlights |
| `textDocument/codeAction` | Quick fixes and refactoring suggestions |

### Project Awareness

The LSP server discovers `plc-project.yaml` in the workspace root and provides cross-file analysis:
- Symbols defined in one file are visible in completion and go-to-definition from other files
- Diagnostics account for cross-file dependencies
- Device I/O globals from `_io_map.st` are included in the analysis

## How to Use

The LSP server is started by editor extensions. For VS Code, the companion extension launches it automatically. For other editors:

```bash
# Start the LSP server manually (stdio mode)
st-cli serve
```

Or directly:

```bash
st-runtime lsp   # if using the unified binary
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-grammar` | Incremental parser |
| `st-syntax` | AST and project loading |
| `st-semantics` | Semantic analysis for diagnostics |
| `tower-lsp` | LSP protocol framework |
| `tokio` | Async runtime |
| `tracing` | Logging |
