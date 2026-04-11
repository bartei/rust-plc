# Development Setup

This guide covers how to build, run, and develop the rust-plc project.

## Prerequisites

- **Rust 1.85+** (edition 2024). Install via [rustup](https://rustup.rs/).
- **Node.js LTS** (for the VSCode extension). The devcontainer installs this
  automatically.
- **C compiler** (for building tree-sitter). Usually available by default on
  Linux/macOS; on Windows use MSVC.

## Clone and Build

```bash
git clone <repository-url>
cd rust-plc

# Build the CLI (and all dependencies)
cargo build -p st-cli

# Build the entire workspace
cargo build --workspace
```

The primary binary is `st-cli`, which serves as both the command-line tool and
the LSP server process.

## Run Tests

```bash
# Run all tests across every crate
cargo test --workspace

# Run tests for a specific crate
cargo test -p st-grammar
cargo test -p st-semantics

# Run a single test by name
cargo test -p st-engine test_arithmetic
```

## Project Structure

```
rust-plc/
  Cargo.toml                  Workspace root (10 members)
  crates/
    st-grammar/               Tree-sitter parser wrapper
      src/lib.rs              language(), kind constants, grammar tests
    st-syntax/                AST definitions + CST-to-AST lowering
      src/ast.rs              Typed AST nodes (SourceFile, Statement, Expr...)
      src/lower.rs            Tree-sitter walk -> AST construction
      src/lib.rs              parse() convenience function
      tests/
        lower_tests.rs        AST lowering tests
        coverage_gaps.rs      Additional coverage tests
    st-semantics/             Semantic analysis
      src/analyze.rs          Two-pass analyzer
      src/scope.rs            Hierarchical symbol table
      src/types.rs            Ty enum, coercion rules, numeric ranking
      src/diagnostic.rs       Diagnostic codes and severities
      src/lib.rs              check() convenience function
      tests/
        end_to_end_tests.rs   Full parse-analyze round trips
        type_tests.rs         Type checking
        scope_tests.rs        Scope resolution
        call_tests.rs         Function/FB call validation
        control_flow_tests.rs IF/FOR/WHILE/CASE checks
        struct_array_tests.rs UDT tests
        warning_tests.rs      Unused variable warnings etc.
        coverage_gaps.rs      Additional coverage
        test_helpers.rs       Shared test utilities
    st-ir/                    Intermediate representation
      src/lib.rs              Module, Function, Instruction, Value, MemoryLayout
    st-compiler/              AST -> IR compilation
      src/compile.rs          ModuleCompiler + FunctionCompiler
      src/lib.rs              compile() public API
      tests/
        compile_tests.rs      Compilation tests
    st-engine/                Bytecode VM + scan-cycle engine
      src/vm.rs               Vm, CallFrame, fetch-decode-execute loop
      src/engine.rs           Engine, CycleStats, watchdog
      src/lib.rs              Public re-exports
      tests/
        vm_tests.rs           VM execution tests
    st-lsp/                   Language Server Protocol
      src/server.rs           tower-lsp Backend implementation
      src/document.rs         Per-document state (tree, AST, analysis)
      src/completion.rs       Completion provider
      src/semantic_tokens.rs  Semantic token encoding
      src/lib.rs              run_stdio()
      tests/
        lsp_integration.rs    Subprocess-based LSP tests (13 tests)
        unit_tests.rs         In-process LSP tests (41 tests)
    st-dap/                   Debug Adapter Protocol (placeholder)
      src/lib.rs
    st-monitor/               WebSocket live monitoring (placeholder)
      src/lib.rs
    st-cli/                   CLI entry point
      src/main.rs             serve, check, run commands
  editors/
    vscode/                   VSCode extension
      package.json            Extension manifest
      src/extension.ts        Thin client: launches st-cli serve
      syntaxes/               TextMate grammar for highlighting
      language-configuration.json
  docs/                       mdBook documentation (you are here)
  .devcontainer/              Development container definition
  playground/                 Example .st files for testing
```

## Using the Devcontainer

The project includes a devcontainer configuration in `.devcontainer/`:

- **Dockerfile** builds an image with Rust and Node.js.
- **devcontainer.json** configures VSCode with rust-analyzer and ST file
  associations.
- **post-create.sh** runs after container creation to install dependencies.

To use it:

1. Install the "Dev Containers" extension in VSCode.
2. Open the project folder.
3. VSCode will prompt "Reopen in Container" -- accept.
4. The container will build and configure itself automatically.

## Launching the Extension Development Host

To test the VSCode extension with the LSP server:

1. Open the project in VSCode.
2. Make sure the Rust binary is built: `cargo build -p st-cli`.
3. Open the `editors/vscode/` folder in VSCode (or use the workspace).
4. Press **F5** to launch the Extension Development Host.
5. In the new VSCode window, open a `.st` file. The extension will launch
   `st-cli serve` and connect to it over stdio.

The server path is configured via `structured-text.serverPath` in settings.
The devcontainer sets this to `${workspaceFolder}/target/debug/st-cli`.

## Useful Commands

```bash
# Type check without running
cargo build --workspace 2>&1 | head -20

# Check a Structured Text file for errors
cargo run -p st-cli -- check playground/example.st

# Run a Structured Text program for 10 scan cycles
cargo run -p st-cli -- run playground/example.st -n 10

# Start the LSP server manually (for debugging)
cargo run -p st-cli -- serve

# Format all code
cargo fmt --all

# Run clippy lints
cargo clippy --workspace

# Build in release mode
cargo build --workspace --release
```

## IDE Support

- **CLion / RustRover** -- Open the workspace `Cargo.toml`. The IDE will
  index all 10 crates automatically.
- **VSCode with rust-analyzer** -- Same; open the root folder. The
  devcontainer pre-configures this.
- The workspace uses `resolver = "3"` (Rust 2024 edition) so all crates
  share a single dependency graph.
