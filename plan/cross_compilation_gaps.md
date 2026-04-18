# Cross-Compilation Gaps & Missing Features

> Status as of 2026-04-18: aarch64 cross-compilation works end-to-end.
> This document tracks known issues, workarounds, and improvements needed.

## What works

- [x] x86_64 static musl binary (compiled + tested on QEMU VM)
- [x] aarch64 static musl binary (cross-compiled + tested on QEMU VM)
- [x] Native FB projects compile, bundle, deploy, and run on both architectures
- [x] Variable catalog and monitoring work on both architectures
- [x] E2E tests pass for both x86_64 (21/21) and aarch64 (4/4)

## Build requirements

### x86_64
```bash
rustup target add x86_64-unknown-linux-musl
nix-shell -p pkgsCross.musl64.stdenv.cc --run \
  "CC_x86_64_unknown_linux_musl=x86_64-unknown-linux-musl-gcc \
   cargo build -p st-target-agent -p st-cli \
   --target x86_64-unknown-linux-musl --profile release-static"
```

### aarch64
```bash
rustup target add aarch64-unknown-linux-musl
nix-shell -p pkgsCross.aarch64-multiplatform-musl.stdenv.cc --run \
  "CC_aarch64_unknown_linux_musl=aarch64-unknown-linux-musl-gcc \
   CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-unknown-linux-musl-gcc \
   cargo build -p st-target-agent -p st-cli \
   --target aarch64-unknown-linux-musl --profile release-static"
```

**Note:** The aarch64 build requires `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER` to be set
explicitly. Without it, cargo uses the host linker which can't link aarch64 object files. The
x86_64 build doesn't need this because the nix cross-compiler wrapper handles it.

## Known gaps

### 1. `build-static.sh` doesn't set the linker env var for aarch64

**File:** `scripts/build-static.sh`

The script sets `CC_aarch64_unknown_linux_musl` but not
`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER`. This causes the aarch64 build to fail with
"file in wrong format" linker errors.

**Fix:** Add `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER` to the aarch64 case in the script.

**Severity:** Build blocker for aarch64 targets. Easy fix.

### 2. Agent doesn't build NativeFbRegistry from bundled profiles at runtime

**Current behavior:** The bundle includes device profile YAMLs in its `profiles/` directory.
The agent's runtime manager loads the compiled Module from the bundle and creates a VM, but
does NOT build a `NativeFbRegistry` from the bundled profiles. This means `NativeFb::execute()`
is never called on the target — native FB calls are no-ops at runtime.

**Impact:** Simulated device fields like `connected`, `io_cycles`, `last_response_ms` remain
at their default values (false, 0, 0.0) on the deployed target. The program logic that doesn't
depend on `execute()` (field writes, reads of user-set values) works correctly.

**Fix:** Modify `st-target-agent/src/runtime_manager.rs` to:
1. When loading a bundle, extract profile YAMLs from the stored bundle data
2. Build a `NativeFbRegistry` from those profiles (using `SimulatedNativeFb` for simulated protocol)
3. Pass the registry to `Engine::new_with_native_fbs()`

**Severity:** Functional gap. Programs compile and run, but native device I/O doesn't execute.
Required before real hardware I/O can work on deployed targets.

### 3. No `.cargo/config.toml` for persistent linker configuration

Currently, cross-compilation requires passing env vars on every build. A `.cargo/config.toml`
with target-specific linker settings would make this automatic:

```toml
[target.aarch64-unknown-linux-musl]
linker = "aarch64-unknown-linux-musl-gcc"

[target.x86_64-unknown-linux-musl]
linker = "x86_64-unknown-linux-musl-gcc"
```

**Issue:** These linker paths are only valid inside the nix-shell. Adding them permanently
would break builds outside nix-shell. This could be solved with a wrapper script or by
using cargo's `--config` flag.

**Severity:** Developer ergonomics. Not a blocker.

### 4. E2E tests require manual setup for aarch64

Running aarch64 e2e tests requires:
1. `qemu-system-aarch64` available (via `nix-shell -p qemu`)
2. Debian aarch64 cloud image downloaded (`setup-images.sh aarch64`)
3. UEFI firmware available (copied from nix qemu package)
4. Static aarch64 binaries pre-built
5. Tests run from within `nix-shell -p qemu`

**Improvement:** Add a `scripts/run-e2e.sh` that automates all setup and runs within nix-shell.

**Severity:** Developer ergonomics. Tests work, just require manual setup.

### 5. aarch64 emulation is slow (~10x slower than KVM)

The QEMU aarch64 emulation runs without KVM hardware acceleration. Boot takes ~35s, agent
startup ~23s, and each test takes ~90s. The full aarch64 suite takes ~6 minutes.

On real ARM64 hardware (or with KVM on an ARM64 host), performance would be comparable to x86_64.

**Mitigation:** aarch64 tests are gated behind `ST_E2E_AARCH64=1` so they don't slow down
normal development. Run them before releases or in nightly CI on ARM64 runners.

**Severity:** Not a bug. Expected behavior for cross-architecture emulation.

### 6. `start-vm.sh` had `-nographic -daemonize` conflict for aarch64

**Fixed:** Changed to `-display none -daemonize` (same as x86_64). The `-nographic` flag
conflicts with `-daemonize` in QEMU because `-nographic` redirects serial to stdio which
can't work in daemon mode.

## Future targets

| Architecture | Status | Notes |
|-------------|--------|-------|
| x86_64 | Working | KVM-accelerated QEMU, full e2e tests |
| aarch64 | Working | Emulated QEMU, full e2e tests, ~10x slower |
| riscv64 | Not started | Would need RISC-V QEMU + cloud image |
| armv7 (32-bit) | Not started | Would need `armv7-unknown-linux-musleabihf` target |
