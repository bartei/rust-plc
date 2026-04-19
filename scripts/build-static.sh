#!/bin/bash
# Build static musl binaries for target deployment.
#
# Produces fully statically linked ELF binaries with zero runtime dependencies.
# Runs on any Linux distro (Debian, Ubuntu, Alpine, etc.) regardless of glibc.
#
# The .cargo/config.toml linker wrappers automatically invoke nix-shell to
# get the musl cross-compiler, so this script just needs to enter a nix-shell
# with the CC compiler available (for tree-sitter C code compilation).
#
# Usage:
#   ./scripts/build-static.sh              # Build x86_64 (default)
#   ./scripts/build-static.sh aarch64      # Build aarch64 (ARM64)
#
# Prerequisites:
#   - Rust musl targets: rustup target add x86_64-unknown-linux-musl
#   - Nix package manager (provides musl cross-compiler)
#
# Output:
#   target/<target>/release-static/st-target-agent
#   target/<target>/release-static/st-cli

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

echo "Building for ${TARGET}..."
echo "  Profile: release-static (opt-level=s, LTO, strip, panic=abort)"

# Ensure the musl target is installed
rustup target add "$TARGET" 2>/dev/null || true

# Build inside nix-shell so both the CC compiler and the linker (via
# .cargo/config.toml wrapper) have the musl toolchain available.
nix-shell -p "$NIX_PKG" --run \
    "${CC_VAR}=${CC_BIN} cargo build \
        -p st-target-agent -p st-cli \
        --target ${TARGET} \
        --profile release-static"

echo ""
for BIN_NAME in st-target-agent st-cli; do
    BINARY="target/${TARGET}/release-static/${BIN_NAME}"
    if [ -f "$BINARY" ]; then
        SIZE=$(ls -lh "$BINARY" | awk '{print $5}')
        FILE_INFO=$(file "$BINARY" | grep -oE "static(-pie)? linked" || echo "WARNING: not static!")
        echo "  ${BIN_NAME}: ${SIZE} (${FILE_INFO})"
    fi
done
echo ""
echo "Deploy with: st-cli target install user@host"
