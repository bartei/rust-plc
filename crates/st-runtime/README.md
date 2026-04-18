# st-runtime

Unified PLC runtime binary for target deployment.

## Purpose

Single statically-linked binary that combines the target agent, debug server, compiler, and runtime into one executable. This is the **only file deployed to production targets** — no dependencies, no installers, no package managers.

## How to Use

### Subcommands

```bash
# Run as agent daemon (production mode — systemd starts this)
st-runtime agent --config /etc/st-plc/agent.yaml

# Compile and run a program directly (development/testing)
st-runtime run ./my-project
st-runtime run main.st --cycles 100

# Check syntax and semantics
st-runtime check ./my-project

# Start DAP debug server (called internally by agent for remote debug)
st-runtime debug ./my-project

# Print version and target information
st-runtime version
```

### Agent Mode (Production)

The primary mode. Started by systemd on boot, it:
1. Loads configuration from `/etc/st-plc/agent.yaml`
2. Initializes structured logging (systemd journal or file)
3. Acquires a singleton lock (prevents multiple instances)
4. Starts the HTTP REST API on the configured port
5. Starts the DAP proxy for remote debugging
6. Starts the OPC-UA server (if enabled)
7. Auto-starts the deployed program (if configured)
8. Runs until SIGTERM (graceful shutdown)

### Run Mode (Development)

For quick local testing without the full agent:
```bash
st-runtime run . --cycles 1000
```

This compiles the project, sets up simulated devices, and runs the scan cycle.

## Configuration

See [st-target-agent](../st-target-agent/README.md) for the full `agent.yaml` reference.

## Building

### Development

```bash
cargo build -p st-runtime
```

### Production (static binary)

```bash
# x86_64
cargo build --release --target x86_64-unknown-linux-musl -p st-runtime

# ARM (Raspberry Pi)
cargo build --release --target aarch64-unknown-linux-musl -p st-runtime
```

The release-static profile optimizes for size:

```bash
cargo build --profile release-static --target x86_64-unknown-linux-musl -p st-runtime
```

### Without OPC-UA (smaller binary)

```bash
cargo build --release --no-default-features -p st-runtime
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-target-agent` | Agent daemon (HTTP, DAP proxy, runtime manager) |
| `st-dap` | Debug adapter server |
| `st-syntax` through `st-engine` | Full compilation + execution pipeline |
| `st-comm-api` | Communication framework |
| `clap` | CLI parsing |
| `tokio` | Async runtime |
| `axum` | HTTP server |

## Production Deployment

### Installation

```bash
# From developer machine
st-cli target install plc@192.168.1.50
```

This:
1. Connects via SSH
2. Detects target architecture
3. Uploads the correct static binary (~4 MB)
4. Creates directories (`/etc/st-plc/`, `/var/lib/st-plc/`, `/var/log/st-plc/`)
5. Writes default `agent.yaml`
6. Installs systemd service
7. Starts the agent and verifies health

### Upgrade

```bash
st-cli target install plc@192.168.1.50 --upgrade
```

Preserves configuration and deployed programs.

### Service Management

```bash
# On the target
sudo systemctl status st-runtime
sudo systemctl restart st-runtime
sudo journalctl -u st-runtime -f    # live logs
```

### Filesystem Layout

```
/usr/local/bin/st-runtime            # Static binary
/etc/st-plc/agent.yaml              # Agent configuration
/var/lib/st-plc/programs/           # Deployed program bundles
/var/lib/st-plc/retain/             # Retained variables + OPC-UA PKI
/var/log/st-plc/                    # Log files
/run/st-runtime/st-runtime.pid      # Singleton lock
```
