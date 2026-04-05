# Introduction

**rust-plc** is an open-source IEC 61131-3 Structured Text compiler toolchain built in Rust. It provides everything you need to write, analyze, and execute PLC programs:

- **Compiler** — Parses Structured Text source code, performs semantic analysis with 30+ diagnostic checks, and compiles to register-based bytecode
- **Runtime** — A bytecode VM that executes compiled programs in a PLC-style scan cycle loop
- **LSP Server** — Full Language Server Protocol integration for real-time diagnostics, hover information, go-to-definition, code completion, and syntax highlighting in VSCode
- **CLI Tool** — `st-cli` provides check, compile, and run commands for the terminal

## What is Structured Text?

Structured Text (ST) is one of the five programming languages defined by the IEC 61131-3 standard for programmable logic controllers (PLCs). It's a high-level, Pascal-like language used extensively in industrial automation:

```st
PROGRAM TemperatureControl
VAR
    sensor_temp : REAL := 22.0;
    setpoint    : REAL := 50.0;
    heater_on   : BOOL := FALSE;
END_VAR
    IF sensor_temp < setpoint - 2.0 THEN
        heater_on := TRUE;
    ELSIF sensor_temp > setpoint + 2.0 THEN
        heater_on := FALSE;
    END_IF;
END_PROGRAM
```

## Key Features

| Feature | Status |
|---------|--------|
| Full ST parser with error recovery | ✅ |
| 30+ semantic diagnostics (type errors, undeclared vars, etc.) | ✅ |
| LSP server (diagnostics, hover, go-to-def, completion) | ✅ |
| VSCode extension with syntax highlighting | ✅ |
| Bytecode compiler | ✅ |
| Runtime VM with scan cycle engine | ✅ |
| Debugger (DAP) | 🔜 |
| Online change (hot reload) | 🔜 |
| LLVM native compilation | 🔜 |

## Quick Example

```bash
# Check a file for errors
st-cli check program.st

# Compile and run for 1000 scan cycles
st-cli run program.st -n 1000

# Start the LSP server (used by VSCode)
st-cli serve
```

## Architecture

The toolchain follows the same architecture as [rust-analyzer](https://rust-analyzer.github.io/): a Rust core process handles all the heavy lifting, while a thin TypeScript extension bridges to VSCode.

```
Source (.st) → Parser → AST → Semantics → IR → VM
                              ↓
                          LSP Server → VSCode
```

Continue to [Installation](./getting-started/installation.md) to get started.
