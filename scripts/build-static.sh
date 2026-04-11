#!/bin/bash
# Build static musl binary of st-runtime for target deployment.
#
# Produces a fully statically linked ELF binary with zero runtime dependencies.
# Runs on any Linux distro (Debian, Ubuntu, Alpine, etc.) regardless of glibc version.
#
# Usage:
#   ./scripts/build-static.sh              # Build x86_64 (default)
#   ./scripts/build-static.sh aarch64      # Build aarch64 (ARM64)
#
# Prerequisites:
#   - Rust with musl target: rustup target add x86_64-unknown-linux-musl
#   - Nix package manager (for musl cross-compiler)
#
# Output:
#   target/x86_64-unknown-linux-musl/release-static/st-runtime

set -euo pipefail

ARCH="${1:-x86_64}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

case "$ARCH" in
    x86_64)
        TARGET="x86_64-unknown-linux-musl"
        NIX_PKG="pkgsCross.musl64.stdenv.cc"
        CC_VAR="CC_x86_64_unknown_linux_musl"
        CC_BIN="x86_64-unknown-linux-musl-gcc"
        ;;
    aarch64)
        TARGET="aarch64-unknown-linux-musl"
        NIX_PKG="pkgsCross.aarch64-multiplatform-musl.stdenv.cc"
        CC_VAR="CC_aarch64_unknown_linux_musl"
        CC_BIN="aarch64-unknown-linux-musl-gcc"
        ;;
    *)
        echo "Usage: $0 [x86_64|aarch64]"
        exit 1
        ;;
esac

echo "Building st-runtime for ${TARGET}..."
echo "  Profile: release-static (opt-level=s, LTO, strip, panic=abort)"

# Ensure the musl target is installed
rustup target add "$TARGET" 2>/dev/null || true

# Build with nix-provided musl cross-compiler
nix-shell -p "$NIX_PKG" --run \
    "${CC_VAR}=${CC_BIN} cargo build \
        -p st-runtime \
        --target ${TARGET} \
        --profile release-static"

BINARY="target/${TARGET}/release-static/st-runtime"

if [ -f "$BINARY" ]; then
    SIZE=$(ls -lh "$BINARY" | awk '{print $5}')
    FILE_INFO=$(file "$BINARY" | grep -oE "static(-pie)? linked" || echo "WARNING: not static!")
    echo ""
    echo "Success: ${BINARY}"
    echo "  Size: ${SIZE}"
    echo "  Type: ${FILE_INFO}"
    echo ""
    echo "Deploy with: st-cli target install user@host"
else
    echo "ERROR: Binary not found at ${BINARY}"
    exit 1
fi
