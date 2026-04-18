# st-cli

Command-line interface for the IEC 61131-3 PLC toolchain.

## Purpose

User-facing CLI for local development: check, compile, run, debug, format, bundle, and deploy Structured Text programs. Also starts the LSP and DAP servers for editor integration.

## Installation

The `st-cli` binary is built from the workspace:

```bash
cargo build --release -p st-cli
```

## Commands

### Development

```bash
# Check syntax and semantics (report errors)
st-cli check .
st-cli check main.st
st-cli check . --json              # JSON output for CI

# Run a program locally
st-cli run .                       # run until stopped (Ctrl+C)
st-cli run . -n 100                # run 100 scan cycles

# Compile to bytecode
st-cli compile main.st -o out.stc

# Format source files
st-cli fmt .

# Start local debug session (DAP over stdio)
st-cli debug .
```

### Editor Integration

```bash
# Start Language Server (stdio mode — called by editor extensions)
st-cli serve
```

### Deployment

```bash
# Create a program bundle for remote deployment
st-cli bundle .                    # development bundle (includes source)
st-cli bundle . --release          # release bundle (no source, stripped debug)
st-cli bundle . --release-debug    # release with obfuscated debug info

# List configured targets
st-cli target list .

# Install/upgrade runtime on target
st-cli target install plc@192.168.1.50

# Regenerate I/O map from device profiles
st-cli comm-gen .
```

### Path Resolution

| Input | Mode |
|-------|------|
| No argument | Current directory (project mode) |
| `file.st` | Single file |
| `directory/` | Project mode (discovers `plc-project.yaml` + `*.st` files) |
| `plc-project.yaml` | Explicit project file |

## Configuration

Projects use `plc-project.yaml`:

```yaml
name: MyProject
entryPoint: Main
engine:
  cycle_time: 10ms
links:
  - name: sim_link
    type: simulated
devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    device_profile: sim_8di_4ai_4do_2ao
targets:
  - name: my-plc
    host: 192.168.1.50
    user: plc
```

## Functional Description

When running a program (`st-cli run`), the CLI:

1. Discovers the project (YAML or directory scan)
2. Loads communication config, generates `_io_map.st` if devices are configured
3. Parses all source files (including `_io_map.st` and stdlib)
4. Runs semantic analysis and reports any errors
5. Compiles to bytecode
6. Creates an `Engine` with the configured cycle time
7. Registers simulated communication devices
8. Starts device web UIs (on ports 8080+)
9. Runs the scan cycle loop

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-grammar` through `st-engine` | Full compilation + execution pipeline |
| `st-lsp` | Language server |
| `st-dap` | Debug adapter |
| `st-monitor` | Variable monitoring |
| `st-comm-api`, `st-comm-sim` | Communication I/O |
| `st-deploy` | Program bundling and deployment |
| `tokio` | Async runtime |
| `tracing`, `tracing-subscriber` | Logging |
| `anyhow` | Error handling |

## Production Deployment

`st-cli` is a **development tool** — it is NOT deployed to production targets. For production, `st-runtime` (the unified binary) is used instead. The CLI is for the developer's machine: editing, checking, running locally, bundling, and deploying to targets.
