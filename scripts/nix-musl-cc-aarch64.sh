#!/usr/bin/env bash
# Linker wrapper for aarch64-unknown-linux-musl.
# Called by cargo via .cargo/config.toml.
#
# If the musl-gcc is already in PATH (e.g., inside a nix-shell), use it directly.
# Otherwise, invoke nix-shell to get it. This avoids the nix-shell startup
# overhead when already in the right environment.
set -euo pipefail

CC="aarch64-unknown-linux-musl-gcc"

if command -v "$CC" &>/dev/null; then
    exec "$CC" "$@"
else
    exec nix-shell -p pkgsCross.aarch64-multiplatform-musl.stdenv.cc --run "$CC $*"
fi
