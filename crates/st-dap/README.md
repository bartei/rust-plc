# st-dap

Debug Adapter Protocol server for online PLC debugging.

## Purpose

Implements the Debug Adapter Protocol (DAP) to enable IDE-based debugging of running PLC programs. Supports breakpoints, stepping, variable inspection, force/unforce, and watch expressions — both locally and on remote targets.

## Public API

```rust
use st_dap::run_dap;

// Start DAP server over stdin/stdout
run_dap(std::io::stdin(), std::io::stdout(), project_path);
```

- `run_dap(reader, writer, path)` — Starts the DAP server with the given I/O streams and project path

## Functional Description

### Supported Debug Features

| Feature | Description |
|---------|-------------|
| **Breakpoints** | Set/clear line breakpoints in any source file |
| **Stepping** | Step In, Step Over, Step Out |
| **Pause/Continue** | Pause at any point, resume execution |
| **Variables** | Inspect local, global, and FB instance variables |
| **Watch** | Evaluate expressions in the Watch panel |
| **Force/Unforce** | Override variable values during execution |
| **Call Stack** | View the current call stack with source locations |
| **Multi-file** | Breakpoints and stepping work across file boundaries |

### Debug Modes

- **Local debug** — Compiles and runs the program in a local VM with the DAP server attached. Started by `st-cli debug <path>` or VS Code launch configuration.
- **Remote debug** — Attaches to a running program on a remote target via the agent's DAP proxy. The agent spawns a DAP session and proxies the protocol over TCP.

### Communication Setup

The DAP server sets up simulated communication devices (from `plc-project.yaml`) so I/O variables are available during debug. Device web UIs are started on their configured ports for interactive I/O testing.

## How to Use

### VS Code (recommended)

Create a `.vscode/launch.json`:

```jsonc
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "st",
            "request": "launch",
            "name": "Debug Local",
            "program": "${workspaceFolder}"
        },
        {
            "type": "st",
            "request": "attach",
            "name": "Debug Remote",
            "target": "my-plc"
        }
    ]
}
```

### CLI

```bash
# Local debugging (DAP over stdio)
st-cli debug .

# The agent's DAP proxy listens on port 4841 for remote attach
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-engine` | VM and engine for local execution |
| `st-ir` | Bytecode types |
| `st-syntax`, `st-compiler` | Compilation for local debug |
| `st-comm-api`, `st-comm-sim` | I/O device setup |
| `st-monitor` | Variable monitoring during debug |
| `dap` | DAP protocol types |
| `tokio` | Async runtime |
