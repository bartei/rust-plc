# VSCode Setup

The rust-plc toolchain includes a VSCode extension that provides full IDE support for Structured Text.

## Features

- **Syntax highlighting** — Keywords, types, variables, comments, literals
- **Real-time diagnostics** — Errors and warnings as you type
- **Hover information** — Type info for variables, function signatures
- **Go-to-definition** — Ctrl+Click to jump to declarations
- **Code completion** — Variables, functions, keywords, struct fields after `.`
- **Document outline** — Symbol tree in the sidebar
- **Semantic tokens** — Context-aware highlighting (distinguishes functions from variables)

## Installation

### Option A: Devcontainer (Recommended)

1. Open the `rust-plc` repository in VSCode
2. Click "Reopen in Container" or run `Dev Containers: Reopen in Container`
3. Everything is configured automatically

### Option B: Extension Development Host

1. Build the CLI and extension:
   ```bash
   cargo build -p st-cli
   cd editors/vscode && npm install && npm run compile
   ```

2. Press **F5** in VSCode (with the rust-plc repo open)
3. Select **"Launch Extension (playground)"**
4. A new VSCode window opens with the extension loaded and the `playground/` folder open

### Option C: Manual Installation

1. Build `st-cli`:
   ```bash
   cargo build -p st-cli --release
   ```

2. Package the extension:
   ```bash
   cd editors/vscode
   npm install
   npm run compile
   npx @vscode/vsce package --no-dependencies
   ```

3. Install the `.vsix`:
   ```bash
   code --install-extension iec61131-st-0.1.0.vsix
   ```

4. Configure the server path in VSCode settings:
   ```json
   {
     "structured-text.serverPath": "/path/to/st-cli"
   }
   ```

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `structured-text.serverPath` | `st-cli` | Path to the `st-cli` binary |

## File Associations

The extension automatically activates for files with these extensions:
- `.st` — Structured Text
- `.scl` — Structured Control Language (Siemens variant)

## Troubleshooting

### Extension not activating
- Check that `st-cli` is built and accessible
- Open the Output panel → select "Structured Text Language Server"
- Verify the binary path in settings

### No syntax highlighting
- Reload the window: Ctrl+Shift+P → "Developer: Reload Window"
- Check that the file is recognized as "Structured Text" (bottom-right status bar)

### LSP errors
- Build st-cli: `cargo build -p st-cli`
- Check the Output panel for error messages from the language server
