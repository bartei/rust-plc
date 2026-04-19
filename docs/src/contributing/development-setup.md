# Development Setup

This guide covers how to build, run, and develop the rust-plc project.

## Prerequisites

- **Rust 1.85+** (edition 2024). Install via [rustup](https://rustup.rs/).
- **Nix package manager** — provides reproducible cross-compilation toolchains
  and any CLI tools needed. Install via
  [Determinate Systems installer](https://install.determinate.systems/nix).
- **Node.js LTS** (for the VSCode extension). Available via `nix-shell -p nodejs`
  or installed by the devcontainer automatically.
- **C compiler** (for tree-sitter). Usually available by default on Linux/macOS.

The devcontainer includes all prerequisites. For a native workstation setup,
install Rust and Nix, then everything else is handled by nix-shell on demand.

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

## Cross-Compilation (Static Binaries)

Target deployment requires statically linked musl binaries. The project uses
nix-provided cross-compilers configured in `.cargo/config.toml`:

```bash
# Add musl targets (one-time)
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl

# Build static binaries (nix provides the cross-compiler automatically)
./scripts/build-static.sh              # x86_64
./scripts/build-static.sh aarch64      # ARM64
```

The `.cargo/config.toml` linker wrappers (`scripts/nix-musl-cc-*.sh`) handle
nix-shell invocation transparently. If you're already inside a nix-shell with
the right toolchain, the wrapper uses it directly (no overhead). Otherwise it
spawns a nix-shell per linker invocation.

For faster iterative cross-builds, enter a nix-shell first:

```bash
# x86_64 musl
nix-shell -p pkgsCross.musl64.stdenv.cc --run \
  "CC_x86_64_unknown_linux_musl=x86_64-unknown-linux-musl-gcc \
   cargo build -p st-target-agent --target x86_64-unknown-linux-musl --profile release-static"

# aarch64 musl
nix-shell -p pkgsCross.aarch64-multiplatform-musl.stdenv.cc --run \
  "CC_aarch64_unknown_linux_musl=aarch64-unknown-linux-musl-gcc \
   cargo build -p st-target-agent --target aarch64-unknown-linux-musl --profile release-static"
```

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

For QEMU end-to-end tests, see [Testing](testing.md).

## Using Nix for Tools

Any command-line tool needed during development can be obtained via nix-shell
without installing it system-wide:

```bash
# QEMU for e2e tests
nix-shell -p qemu --run "qemu-system-aarch64 --version"

# Node.js for extension development
nix-shell -p nodejs --run "npm run compile"

# mdBook for documentation
nix-shell -p mdbook --run "mdbook build docs"

# Multiple tools at once
nix-shell -p qemu nodejs mdbook
```

This ensures all developers use the same tool versions regardless of their
host OS or package manager.

## Project Structure

```
rust-plc/
  Cargo.toml                  Workspace root (17 members)
  .cargo/config.toml          Cross-compilation linker wrappers
  crates/
    st-grammar/               Tree-sitter parser wrapper
    st-syntax/                AST definitions + CST-to-AST lowering
    st-semantics/             Semantic analysis, type checking
    st-ir/                    Intermediate representation (bytecode)
    st-compiler/              AST -> IR compilation
    st-engine/                Bytecode VM + scan-cycle engine
    st-lsp/                   Language Server Protocol
    st-dap/                   Debug Adapter Protocol server
    st-monitor/               WebSocket live monitoring
    st-cli/                   CLI entry point (check, run, bundle, serve)
    st-comm-api/              Communication framework (NativeFb trait, profiles)
    st-comm-sim/              Simulated device with web UI
    st-deploy/                Program bundler and deployment
    st-target-agent/          Remote deployment agent
    st-runtime/               Unified runtime binary for targets
    st-opcua-server/          OPC-UA server integration
  editors/
    vscode/                   VSCode extension (LSP client + PLC monitor panel)
  profiles/                   Device profile YAML files
  stdlib/                     IEC 61131-3 standard library (.st files)
  docs/                       mdBook documentation
  scripts/
    build-static.sh           Build static musl binaries via nix
    nix-musl-cc-x86_64.sh    Linker wrapper for x86_64-musl
    nix-musl-cc-aarch64.sh   Linker wrapper for aarch64-musl
  tests/
    e2e-deploy/               QEMU VM end-to-end tests
  playground/                 Example projects
  .devcontainer/              Devcontainer (Rust + Nix + Node.js)
```

## Using the Devcontainer

The project includes a devcontainer in `.devcontainer/` with Rust, Nix, and
Node.js pre-installed:

1. Install the "Dev Containers" extension in VSCode.
2. Open the project folder.
3. VSCode will prompt "Reopen in Container" -- accept.
4. The container builds and configures itself (installs dependencies, builds
   the CLI, sets up the VSCode extension).

The devcontainer includes Nix, so cross-compilation and all nix-shell
commands work out of the box.

## Native Workstation Setup (Without Devcontainer)

If you prefer to develop without a devcontainer (e.g., using CLion/RustRover
or a native terminal):

1. Install Rust 1.85+ via [rustup](https://rustup.rs/)
2. Install Nix via [Determinate Systems](https://install.determinate.systems/nix)
3. Add musl targets: `rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl`
4. Clone and build: `cargo build --workspace`

Nix handles everything else — cross-compilers, QEMU, Node.js, mdBook.
No system packages need to be installed manually.

## Launching the Extension Development Host

To test the VSCode extension with the LSP server:

1. Open the project in VSCode.
2. Build the CLI: `cargo build -p st-cli`.
3. Build the extension: `cd editors/vscode && nix-shell -p nodejs --run "npm install && npm run compile"`
4. Press **F5** to launch the Extension Development Host.
5. Open a `.st` file in the new window. The extension launches `st-cli serve`.

The server path is configured via `structured-text.serverPath` in settings.
The devcontainer sets this to `${workspaceFolder}/target/debug/st-cli`.

## Useful Commands

```bash
# Check a Structured Text file for errors
cargo run -p st-cli -- check playground/01_hello.st

# Run a Structured Text program for 10 scan cycles
cargo run -p st-cli -- run playground/sim_project -n 10

# Start the LSP server manually (for debugging)
cargo run -p st-cli -- serve

# Build static binaries for deployment
./scripts/build-static.sh              # x86_64
./scripts/build-static.sh aarch64      # ARM64

# Format all code
cargo fmt --all

# Run clippy lints
cargo clippy --workspace -- -D warnings

# Build documentation
nix-shell -p mdbook --run "mdbook build docs"
```

## IDE Support

- **CLion / RustRover** — Open the workspace `Cargo.toml`. The IDE indexes
  all crates automatically. For cross-compilation, ensure Nix is in your
  PATH so the `.cargo/config.toml` linker wrappers work.
- **VSCode with rust-analyzer** — Open the root folder. The devcontainer
  pre-configures everything. For native setups, install rust-analyzer.
- The workspace uses `resolver = "3"` (Rust 2024 edition) so all crates
  share a single dependency graph.
