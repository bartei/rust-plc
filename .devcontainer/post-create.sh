#!/usr/bin/env bash
set -euo pipefail

# Ensure we're in the workspace root
cd "$(dirname "$0")/.."

echo "==> Building st-cli (LSP server)..."
# Build to a container-specific target dir to avoid glibc mismatch when the
# host's target/ is shared. The host may have built with a newer glibc.
export CARGO_TARGET_DIR="$(pwd)/target/container"
for attempt in 1 2 3; do
    if cargo build -p st-cli 2>&1; then
        break
    fi
    echo "    Build attempt $attempt failed, retrying in 5s..."
    sleep 5
done

echo "==> Adding st-cli to PATH..."
sudo ln -sf "$(pwd)/target/container/debug/st-cli" /usr/local/bin/st-cli
echo "    Linked target/container/debug/st-cli -> /usr/local/bin/st-cli"

echo "==> Installing VSCode extension dependencies..."
cd editors/vscode
npm install 2>&1
npm run compile 2>&1
npm run build:webview 2>&1
cd ../..

echo "==> Installing ST extension into VSCode..."
# Read version from package.json so the symlink always matches
EXT_VERSION=$(node -p "require('./editors/vscode/package.json').version")
if [ -d "$HOME/.vscode-server/extensions" ]; then
    # Remove any stale version-pinned directories
    rm -rf "$HOME/.vscode-server/extensions/rust-plc.iec61131-st-"*
    EXT_DIR="$HOME/.vscode-server/extensions/rust-plc.iec61131-st-${EXT_VERSION}"
    ln -sf "$(pwd)/editors/vscode" "$EXT_DIR"
    echo "    Linked to $EXT_DIR"
else
    # Try the regular vscode path (non-remote)
    rm -rf "$HOME/.vscode/extensions/rust-plc.iec61131-st-"*
    EXT_DIR="$HOME/.vscode/extensions/rust-plc.iec61131-st-${EXT_VERSION}"
    mkdir -p "$HOME/.vscode/extensions"
    ln -sf "$(pwd)/editors/vscode" "$EXT_DIR"
    echo "    Linked to $EXT_DIR"
fi

echo "==> Verifying st-cli works..."
./target/container/debug/st-cli check playground/01_hello.st 2>&1

echo ""
echo "============================================================"
echo "  IEC 61131-3 ST development environment ready!"
echo ""
echo "  >>> RELOAD THE WINDOW to activate the extension <<<"
echo "  (Ctrl+Shift+P -> 'Developer: Reload Window')"
echo ""
echo "  Then open any .st file in playground/ to start coding."
echo "============================================================"
