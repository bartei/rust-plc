# Cross-Compilation

All cross-compilation gaps have been resolved. This document is kept for
reference on the build requirements.

## Build requirements

Cross-compilation toolchains are provided by Nix and configured in
`.cargo/config.toml`. Static builds work out of the box:

```bash
# Add musl targets (one-time)
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl

# Build static binaries
./scripts/build-static.sh              # x86_64
./scripts/build-static.sh aarch64      # ARM64
```

## CI coverage

Both architectures are tested end-to-end in GitHub Actions CI:

- **x86_64**: KVM-accelerated QEMU, full e2e test suite (~20 min)
- **aarch64**: Software-emulated QEMU, aarch64 e2e tests (~25 min)

## Supported targets

| Architecture | Status | Notes |
|-------------|--------|-------|
| x86_64 | Working | KVM-accelerated QEMU, full e2e + CI |
| aarch64 | Working | Emulated QEMU, full e2e + CI (Raspberry Pi target) |
| riscv64 | Not started | Would need RISC-V QEMU + cloud image |
| armv7 (32-bit) | Not started | Would need `armv7-unknown-linux-musleabihf` target |
