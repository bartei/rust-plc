# Project Guidelines

> This file is the single source of truth for project conventions.
> Do not use the `.claude/projects/*/memory/` system — put everything here.

## Build & Test

- Rust toolchain is managed via Nix or rustup. If `cargo` is not on PATH, use `rustup default stable` or `nix-shell`.
- Building packages that depend on `st-comm-serial` requires `libudev-dev` and `pkg-config`. Set `PKG_CONFIG_PATH` to include the Nix store path for `libudev.pc` if needed.
- The `st-comm-modbus` RTU integration tests require `socat` and must be run with `--test-threads=1` (shared virtual serial ports).
- Always run `cargo clippy -- -D warnings` before committing. CI enforces clippy with deny-all-warnings.
- Always run all relevant unit and integration tests (`cargo test`) after making code changes, before reporting work as complete.

## Git

- Never add Co-Authored-By lines to commit messages.
- Follow the existing commit message style: `feat:`, `fix:`, `refactor:`, etc.

## Architecture

- Communication uses a two-layer model: SerialLink (transport) + Device FBs (protocol). Devices take `link := serial.port`, never duplicate serial config fields.
- The `to_modbus_rtu_device_layout()` in `st-comm-api` is the single source of truth for the Modbus RTU device field layout. Both the runtime (`st-comm-modbus`) and tooling (LSP, DAP) must use it.
- Device I/O runs on background threads via `BusManager` (one thread per serial port). `execute()` must never do blocking I/O on the scan cycle thread.

## Permissions

- Allow cargo check, test, clippy, and build commands.
- Allow find and grep for codebase exploration.
