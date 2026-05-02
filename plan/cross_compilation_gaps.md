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

## TODO: pin the Nix toolchain via flake.nix

Both the host and the devcontainer now use Nix for cross-compilation
(`scripts/nix-musl-cc-*.sh` → `nix-shell -p pkgsCross.…`). Single-user Nix is
installed in the devcontainer Dockerfile; no `/nix` volume mount, no daemon.

What's still missing: every `nix-shell -p pkgsCross.…` call evaluates against
whatever nixpkgs channel the user happens to have configured. Two developers
(or two CI runs at different times) can therefore link with subtly different
musl-gcc versions. Same hazard as the previous Debian-musl-tools approach,
just with a slower drift cadence.

### Goal

A `flake.nix` at the repo root that pins nixpkgs and exposes a `devShell`
plus the cross-compiler toolchains, so every build everywhere uses bit-for-bit
identical compilers.

### Proposed approach

1. Add `flake.nix` + `flake.lock` at the repo root with:
   - Pinned `nixpkgs` rev.
   - Pinned rustc via `oxalica/rust-overlay` (replaces rustup-in-Dockerfile).
   - `pkgsCross.musl64.stdenv.cc`, `pkgsCross.aarch64-multiplatform-musl.stdenv.cc`.
   - `socat`, `qemu`, `pkg-config`, `libudev`, `nodejs_22`, etc.
2. Replace `scripts/nix-musl-cc-*.sh` with direct linker references that
   resolve through `nix develop` / `nix shell` env, so `.cargo/config.toml`
   no longer needs per-target wrapper scripts.
3. Make `nix develop` the primary onboarding path; the devcontainer becomes
   an optional convenience layer that just runs `nix develop` for you.
4. CI: replace ad-hoc `apt-get install` + `rustup target add` with
   `nix develop --command cargo …`.
5. Optionally pre-warm the cross-compiler closure in the Dockerfile build
   (`RUN nix-shell -p pkgsCross.musl64.stdenv.cc --run true`) so the first
   `cargo build` inside a fresh container is fast.

### Non-goals / risks

- Don't block day-to-day work on this. The current setup works and is
  consistent across host + devcontainer.
- First `nix develop` is slow (multi-GB closure). Compare against the
  current devcontainer's ~minute-scale rebuild.
- Don't adopt flakes without pinning `nixpkgs` — unpinned flakes are no
  more reproducible than the current channel-tracking approach.
