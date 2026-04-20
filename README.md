# rust-plc

**An open-source IEC 61131-3 Structured Text compiler, runtime, and IDE toolchain built in Rust.**

[![CI](https://github.com/bartei/rust-plc/actions/workflows/ci.yml/badge.svg)](https://github.com/bartei/rust-plc/actions/workflows/ci.yml)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Docs](https://img.shields.io/badge/docs-mdBook-green)](https://bartei.github.io/rust-plc/)

---

Write, analyze, compile, run, and debug PLC programs in [Structured Text](https://en.wikipedia.org/wiki/Structured_text) — all from your terminal or VSCode. Connect to real hardware via Modbus RTU, deploy to embedded targets, and monitor live variables over WebSocket.

```st
PROGRAM TemperatureControl
VAR
    serial  : SerialLink;
    sensor  : PT100Module;
    heater  : BOOL := FALSE;
END_VAR
    serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);
    sensor(link := serial.port, slave_id := 1, refresh_rate := T#100ms);

    IF sensor.TEMP_0 < 48.0 THEN
        heater := TRUE;
    ELSIF sensor.TEMP_0 > 52.0 THEN
        heater := FALSE;
    END_IF;
END_PROGRAM
```

## Features

| Feature | Description |
|---------|-------------|
| **Compiler** | Full ST parser with error recovery, 30+ semantic diagnostics, register-based bytecode compiler |
| **Runtime** | Bytecode VM with PLC scan cycle engine, configurable cycle time, global initialization |
| **Standard Library** | Counters (CTU/CTD/CTUD), timers (TON/TOF/TP), edge detection (R_TRIG/F_TRIG), math, trig, type conversions |
| **LSP Server** | 16 language features: diagnostics, completion, hover, go-to-def, references, rename, formatting, and more |
| **DAP Debugger** | Breakpoints, stepping, variable inspection, force/unforce, scan-cycle-aware continue, remote attach |
| **Device Communication** | Modbus RTU over RS-485/RS-232 with YAML device profiles, batched register I/O, non-blocking async bus threads |
| **OOP** | Classes, interfaces, properties, inheritance, virtual dispatch — full IEC 61131-3 OOP |
| **Online Change** | Hot-reload programs without stopping — variable state migrated automatically |
| **Monitor Server** | WebSocket-based live variable dashboard with force/unforce support |
| **Remote Deployment** | Bundle, upload, and manage programs on embedded Linux targets (Raspberry Pi, x86_64, aarch64) |
| **OPC-UA Server** | OPC-UA variable browsing and read/write for SCADA integration |
| **Multi-file Projects** | Autodiscovery of `.st` files, optional `plc-project.yaml` configuration |
| **Pointers** | `REF_TO`, `REF()`, `^` dereference, `NULL` — full IEC 61131-3 pointer support |
| **RETAIN/PERSISTENT** | Variable persistence across power cycles with automatic checkpointing |

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.85+
- [Node.js](https://nodejs.org/) 18+ (only for the VSCode extension)
- `libudev-dev` and `pkg-config` (Linux, for serial port support)

### Build

```bash
git clone https://github.com/bartei/rust-plc.git
cd rust-plc
cargo build -p st-cli --release
```

### Run a Program

```bash
# Check for errors
./target/release/st-cli check playground/01_hello.st

# Run for 1000 scan cycles
./target/release/st-cli run playground/01_hello.st -n 1000

# Run a multi-file project (autodiscover from current directory)
cd playground/multi_file_project
../../target/release/st-cli run -n 100
```

### Format Code

```bash
./target/release/st-cli fmt program.st
```

### Compile to Bytecode

```bash
./target/release/st-cli compile program.st -o program.json
```

### JSON Diagnostics (for CI)

```bash
./target/release/st-cli check program.st --json
```

## VSCode Extension

The fastest way to get started is with the included **devcontainer**:

1. Open this repo in VSCode
2. Click **"Reopen in Container"**
3. Open any `.st` file in `playground/`

You get syntax highlighting, real-time diagnostics, code completion, debugging, and all 16 LSP features out of the box.

See the [VSCode Setup Guide](https://bartei.github.io/rust-plc/getting-started/vscode-setup.html) for manual installation.

## Documentation

Full documentation is available at **[bartei.github.io/rust-plc](https://bartei.github.io/rust-plc/)**:

- [Installation](https://bartei.github.io/rust-plc/getting-started/installation.html)
- [Quick Start](https://bartei.github.io/rust-plc/getting-started/quickstart.html)
- [VSCode Tutorial](https://bartei.github.io/rust-plc/getting-started/vscode-tutorial.html)
- [Language Reference](https://bartei.github.io/rust-plc/language/program-structure.html)
- [Standard Library](https://bartei.github.io/rust-plc/language/standard-library.html)
- [Device Communication](https://bartei.github.io/rust-plc/communication/overview.html)
- [CLI Reference](https://bartei.github.io/rust-plc/cli/commands.html)
- [Deployment & Remote Management](https://bartei.github.io/rust-plc/deployment/overview.html)
- [Architecture](https://bartei.github.io/rust-plc/architecture/overview.html)

## Project Structure

```
rust-plc/
├── crates/
│   ├── st-grammar/        Tree-sitter ST parser
│   ├── st-syntax/         AST types, CST→AST lowering, project discovery
│   ├── st-semantics/      Type checking, symbol tables, diagnostics
│   ├── st-ir/             Bytecode instruction set (50+ instructions)
│   ├── st-compiler/       AST → bytecode compiler
│   ├── st-engine/         VM, scan cycle engine, online change, debug hooks
│   ├── st-lsp/            Language Server Protocol (16 features)
│   ├── st-dap/            Debug Adapter Protocol (local + remote attach)
│   ├── st-monitor/        WebSocket monitor server
│   ├── st-deploy/         Bundle creation and deployment
│   ├── st-target-agent/   Embedded target agent (program lifecycle, HTTP API)
│   ├── st-runtime/        Runtime binary for target devices
│   ├── st-comm-api/       Communication framework API (NativeFb, profiles)
│   ├── st-comm-serial/    RS-485/RS-232 serial link + bus manager
│   ├── st-comm-modbus/    Modbus RTU protocol (FC01–FC10, batched I/O)
│   ├── st-comm-sim/       Simulated devices with web UI
│   ├── st-opcua-server/   OPC-UA server for SCADA integration
│   └── st-cli/            CLI entry point (check, run, compile, fmt, bundle)
├── stdlib/                Standard library (.st files, auto-included)
├── profiles/              Shared device profiles (Modbus register maps)
├── editors/vscode/        VSCode extension (TypeScript)
├── playground/            Example programs and demo projects
├── tests/                 E2E deployment tests (QEMU x86_64 + aarch64)
├── schemas/               JSON schemas (device profiles)
└── docs/                  mdBook documentation
```

## IEC 61131-3 References

- [IEC 61131-3 Official Standard](https://webstore.iec.ch/publication/4552) — the official specification (Ed.3, paywalled)
- [Fernhill IEC 61131-3 Reference](https://www.fernhillsoftware.com/help/iec-61131-3/common-elements/index.html) — free online reference for syntax and examples
- [PLCopen](https://plcopen.org/iec-61131-3) — industry consortium and supplementary guidelines

## Testing

```bash
# Run all 1170+ tests
cargo test --workspace

# Run with coverage
cargo llvm-cov --workspace --html
```

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

This project is licensed under the **GNU General Public License v3.0** — see the [LICENSE](LICENSE) file for details.

This means you are free to use, modify, and distribute this software, provided that any derivative works are also licensed under the GPL-3.0. See [gnu.org/licenses/gpl-3.0](https://www.gnu.org/licenses/gpl-3.0.en.html) for the full terms.
