# VSCode Setup

The rust-plc toolchain includes a VSCode extension that provides full IDE support for Structured Text.

## Features

The language server provides 16 LSP features for a full IDE experience:

- **Diagnostics** — Real-time errors and warnings as you type (30+ diagnostic codes)
- **Hover** — Ctrl+hover shows type info, function signatures, and variable kinds
- **Go-to-definition** — Ctrl+Click jumps to variable or POU declarations
- **Go-to-type-definition** — Jumps to the TYPE, STRUCT, or FUNCTION_BLOCK declaration of a variable's type
- **Completion** — Auto-complete with keywords (snippets), variables, functions, struct fields (dot-trigger), FB members, and types
- **Signature help** — Parameter hints on `(` and `,` inside function and FB calls
- **Find all references** — Shift+F12 finds all usages of a symbol (case-insensitive, whole-word)
- **Rename symbol** — F2 renames across all occurrences in the file
- **Document symbols** — Ctrl+Shift+O outline view with nested POUs and variables
- **Workspace symbols** — Ctrl+T search for any POU or type across all open files
- **Document highlight** — Cursor on an identifier highlights all occurrences instantly
- **Folding ranges** — Collapse PROGRAM, FUNCTION, VAR, IF, FOR, WHILE, CASE, and comment blocks
- **Document links** — File paths in comments (e.g., `// see utils.st`) become clickable links
- **Semantic tokens** — 10 token types for rich, context-aware syntax highlighting
- **Formatting** — Shift+Alt+F auto-indents the entire file
- **Code actions** — Ctrl+. quick fix: declare undeclared variable as INT

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

## Next Steps

Once the extension is installed and working, see the complete walkthrough:

**[Editing, Running & Debugging in VSCode](./vscode-tutorial.md)** — step-by-step guide covering hover, completion, diagnostics, running programs, setting breakpoints, stepping through code, and inspecting variables.
