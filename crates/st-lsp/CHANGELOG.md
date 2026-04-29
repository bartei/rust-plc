# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.4](https://github.com/bartei/rust-plc/compare/st-lsp-v0.3.3...st-lsp-v0.3.4) - 2026-04-29

### Added

- array fields in device profiles with fb.field[i] access
- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- add struct variable support + rename st-runtime/st-plc-runtime crates
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.3.3
- phase-1 coverage improvements (65% -> 72%)
- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- *(lsp)* cache project files, no disk I/O on keystroke
- release v0.1.1

## [0.3.3](https://github.com/bartei/rust-plc/compare/st-lsp-v0.3.2...st-lsp-v0.3.3) - 2026-04-27

### Added

- array fields in device profiles with fb.field[i] access
- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- add struct variable support + rename st-runtime/st-plc-runtime crates
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- phase-1 coverage improvements (65% -> 72%)
- release v0.3.2
- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- *(lsp)* cache project files, no disk I/O on keystroke
- release v0.1.1

## [0.3.2](https://github.com/bartei/rust-plc/compare/st-lsp-v0.3.1...st-lsp-v0.3.2) - 2026-04-24

### Added

- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- add struct variable support + rename st-runtime/st-plc-runtime crates
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.3.1
- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- *(lsp)* cache project files, no disk I/O on keystroke
- release v0.1.1

## [0.3.1](https://github.com/bartei/rust-plc/compare/st-lsp-v0.3.0...st-lsp-v0.3.1) - 2026-04-23

### Added

- Modbus TCP protocol support, batched coil writes
- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- add struct variable support + rename st-runtime/st-plc-runtime crates
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.1.3
- release v0.1.2
- add comprehensive README.md for all 16 crates
- *(lsp)* cache project files, no disk I/O on keystroke
- release v0.1.1

## [0.1.3](https://github.com/bartei/rust-plc/compare/st-lsp-v0.1.2...st-lsp-v0.1.3) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- add struct variable support + rename st-runtime/st-plc-runtime crates
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- release v0.1.2
- add comprehensive README.md for all 16 crates
- *(lsp)* cache project files, no disk I/O on keystroke
- release v0.1.1

## [0.1.2](https://github.com/bartei/rust-plc/compare/st-lsp-v0.1.1...st-lsp-v0.1.2) - 2026-04-20

### Added

- two-layer comm architecture with non-blocking async I/O
- legacy comm cleanup, hover for FB types, remote debug variable tree
- native function block communication layer
- add struct variable support + rename st-runtime/st-plc-runtime crates
- monitor tree model, parse error quality, pause fix, VS Code E2E tests
- LSP features, multi-file diagnostic fix, FB debugger tree, UI test framework
- *(lsp)* method go-to-definition and snippet parameter completion
- *(phase12)* implement IEC 61131-3 OOP extensions (Classes)
- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)

### Fixed

- resolve all clippy warnings for Rust 1.95 and fix CI test ordering
- member hover for native FB fields, DAP native FB variable expansion
- resolve all clippy 1.94 warnings (abs_diff, is_some_and, format args, while let)
- *(lsp)* filter diagnostics to only show current file's errors
- *(lsp)* class method dot-completion and stable analysis during edits
- *(lsp)* preserve project context on didChange, add didSave handler
- *(lsp)* go-to-definition correctly navigates to cross-file symbols
- *(lsp)* go-to-definition now works for cross-file symbols
- cross-file go-to-definition and breakpoints in non-main files
- add multi-file project support to DAP debugger and LSP

### Other

- add comprehensive README.md for all 16 crates
- *(lsp)* cache project files, no disk I/O on keystroke
- release v0.1.1

## [0.1.1](https://github.com/bartei/rust-plc/compare/st-lsp-v0.1.0...st-lsp-v0.1.1) - 2026-04-06

### Added

- add documentHighlight, foldingRange, typeDefinition, workspaceSymbol, documentLink
- implement signatureHelp, references, rename, formatting, codeAction
- implement REF_TO pointers with ^ dereference and NULL
- multi-file workspace support with autodiscovery and plc-project.yaml
- IEC 61131-3 Structured Text compiler toolchain (phases 0-7)
