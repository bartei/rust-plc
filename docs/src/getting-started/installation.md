# Installation

## Prerequisites

- **Rust** 1.85 or later ([install via rustup](https://rustup.rs/))
- **Node.js** 18+ (only needed for the VSCode extension)
- **C compiler** (for building the tree-sitter parser — usually pre-installed on Linux/macOS)

## Building from Source

```bash
# Clone the repository
git clone https://github.com/user/rust-plc.git
cd rust-plc

# Build the CLI tool
cargo build -p st-cli --release

# The binary is at:
./target/release/st-cli --help
```

## Verify Installation

```bash
# Check version
st-cli help

# Parse and check a sample file
st-cli check playground/01_hello.st

# Run a program
st-cli run playground/01_hello.st
```

Expected output:
```
playground/01_hello.st: OK
```

```
Executed 1 cycle(s) in 8.5µs (avg 8.5µs/cycle, 16 instructions)
```

## Installing the VSCode Extension

See [VSCode Setup](./vscode-setup.md) for detailed instructions.

## Using the Devcontainer

The easiest way to get started is with the included devcontainer:

1. Open the repository in VSCode
2. Click **"Reopen in Container"** when prompted (or Ctrl+Shift+P → "Dev Containers: Reopen in Container")
3. Wait for the container to build and the post-create script to finish
4. Open any `.st` file in the `playground/` folder

The devcontainer automatically:
- Installs Rust and Node.js
- Builds `st-cli`
- Installs the VSCode extension
- Configures syntax highlighting and LSP
