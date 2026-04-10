#!/usr/bin/env bash
set -euo pipefail

# Ensure we're in the workspace root
cd "$(dirname "$0")/.."

echo "==> Building st-cli (LSP server)..."
# Retry cargo build — container networking may not be ready on first attempt.
for attempt in 1 2 3; do
    if cargo build -p st-cli 2>&1; then
        break
    fi
    echo "    Build attempt $attempt failed, retrying in 5s..."
    sleep 5
done

echo "==> Installing VSCode extension dependencies..."
cd editors/vscode
npm install 2>&1
npm run compile 2>&1
cd ../..

echo "==> Installing ST extension into VSCode..."
# Symlink the extension into the VSCode extensions directory
EXT_DIR="$HOME/.vscode-server/extensions/rust-plc.iec61131-st-0.1.0"
if [ -d "$HOME/.vscode-server/extensions" ]; then
    rm -rf "$EXT_DIR"
    ln -sf "$(pwd)/editors/vscode" "$EXT_DIR"
    echo "    Linked to $EXT_DIR"
else
    # Try the regular vscode path (non-remote)
    EXT_DIR="$HOME/.vscode/extensions/rust-plc.iec61131-st-0.1.0"
    mkdir -p "$HOME/.vscode/extensions"
    rm -rf "$EXT_DIR"
    ln -sf "$(pwd)/editors/vscode" "$EXT_DIR"
    echo "    Linked to $EXT_DIR"
fi

echo "==> Verifying st-cli works..."
./target/debug/st-cli check playground/01_hello.st 2>&1

echo ""
echo "============================================================"
echo "  IEC 61131-3 ST development environment ready!"
echo ""
echo "  >>> RELOAD THE WINDOW to activate the extension <<<"
echo "  (Ctrl+Shift+P -> 'Developer: Reload Window')"
echo ""
echo "  Then open any .st file in playground/ to start coding."
echo "============================================================"
