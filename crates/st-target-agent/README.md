# st-target-agent

PLC runtime agent for remote deployment, lifecycle management, and monitoring.

## Purpose

Standalone daemon that runs on target devices (Linux embedded PCs, Raspberry Pi, industrial IPCs). Manages the PLC runtime lifecycle and exposes HTTP REST, WebSocket, DAP, and OPC-UA interfaces for remote deployment, control, debugging, and HMI integration.

This is the **only component installed on production targets** — everything else (compiler, LSP, CLI) stays on the developer's machine.

## How to Use

### Install on Target

```bash
# From the developer machine:
st-cli target install plc@192.168.1.50
```

This uploads the `st-runtime` binary via SSH, installs a systemd service, and starts the agent.

### Deploy a Program

```bash
# Bundle and upload
st-cli bundle .
curl -X POST -F "file=@MyProject.st-bundle" http://192.168.1.50:4840/api/v1/program/upload

# Start
curl -X POST http://192.168.1.50:4840/api/v1/program/start

# Check status
curl http://192.168.1.50:4840/api/v1/status
```

### Connect HMI via OPC-UA

Point any OPC-UA client to `opc.tcp://192.168.1.50:4842` — all PLC variables are browsable, subscribable, and writable.

## Configuration

Agent configuration lives at `/etc/st-plc/agent.yaml` on the target:

```yaml
agent:
  name: line1-plc
  description: "Bottle filling line controller"

network:
  bind: 0.0.0.0
  port: 4840                     # HTTP REST API
  dap_port: 4841                 # DAP debug proxy

auth:
  mode: token
  token: "your-secret-token"

runtime:
  mode: vm
  auto_start: true               # start program on boot
  watchdog_ms: 100               # restart if cycle exceeds 100ms
  restart_on_crash: true
  restart_delay_ms: 1000
  max_restarts: 5

storage:
  program_dir: /var/lib/st-plc/programs
  retain_dir: /var/lib/st-plc/retain
  log_dir: /var/log/st-plc

security:
  require_signed: false
  trusted_keys: []

opcua_server:
  enabled: true                  # on by default
  port: 4842
  security_policy: None
  anonymous_access: true
  sampling_interval_ms: 100

logging:
  level: info
```

## HTTP REST API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/v1/program/upload` | POST | Upload a `.st-bundle` |
| `/api/v1/program/info` | GET | Current program metadata |
| `/api/v1/program/start` | POST | Start the PLC program |
| `/api/v1/program/stop` | POST | Stop the PLC program |
| `/api/v1/program/restart` | POST | Stop + start |
| `/api/v1/program` | DELETE | Remove deployed program |
| `/api/v1/status` | GET | Runtime state + cycle stats |
| `/api/v1/health` | GET | Health check (200 OK / 503) |
| `/api/v1/target-info` | GET | OS, arch, agent version, uptime |
| `/api/v1/variables/catalog` | GET | Variable names + types |
| `/api/v1/variables` | GET | Current variable values |
| `/api/v1/variables/force` | POST | Force a variable |
| `/api/v1/variables/force/{name}` | DELETE | Remove force |
| `/api/v1/monitor/ws` | GET | WebSocket monitor upgrade |
| `/api/v1/logs` | GET | Query agent logs |
| `/api/v1/log-level` | GET/PUT | View/change log level |

## Port Allocation

| Port | Protocol | Purpose |
|------|----------|---------|
| 4840 | HTTP | REST API + WebSocket monitor |
| 4841 | TCP | DAP debug proxy |
| 4842 | OPC-UA | HMI/SCADA variable access |

## Functional Description

### Runtime Manager

Manages the PLC engine on a dedicated OS thread:
- Start/stop/restart programs
- Cycle statistics (count, timing, jitter)
- Variable snapshots (updated every scan cycle)
- Force/unforce commands
- Debug session attach/detach
- Crash detection + auto-restart with configurable backoff

### Program Store

Persistent storage for deployed program bundles:
- Extracts `.st-bundle` to the program directory
- Tracks current program metadata
- Supports auto-start on boot

### DAP Proxy

Proxies Debug Adapter Protocol connections from remote VS Code to the engine:
- Spawns DAP sessions for attached debuggers
- Single-session enforcement (one debugger at a time)
- Breakpoints, stepping, variable inspection over the network

### OPC-UA Server (feature: `opcua`)

Built-in OPC-UA server exposes all PLC variables:
- Browse, subscribe, write from any OPC-UA client
- Auto-generated self-signed certificate
- See [st-opcua-server](../st-opcua-server/README.md) for details

### Watchdog

Monitors the engine for hangs and crashes:
- Cycle count monitoring (detects stalls)
- Auto-restart with configurable delay and retry limit
- Reset counter after sustained successful operation (60s)

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-engine` | PLC runtime VM |
| `st-ir` | Bytecode types |
| `st-syntax` | Source parsing (for compile-on-target) |
| `st-monitor` | WebSocket variable monitoring |
| `st-deploy` | Bundle extraction |
| `st-comm-api` | Communication framework |
| `st-opcua-server` | OPC-UA server (optional) |
| `axum` | HTTP framework |
| `tokio` | Async runtime |
| `tracing`, `tracing-journald` | Structured logging (systemd journal) |
| `clap` | CLI argument parsing |

## Production Deployment

The agent runs as a systemd service (`st-runtime.service`):

```ini
[Unit]
Description=ST PLC Runtime
After=network.target

[Service]
ExecStart=/usr/local/bin/st-runtime agent --config /etc/st-plc/agent.yaml
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

The binary is statically linked (musl) with zero external dependencies. It runs on any Linux system — x86_64, aarch64, armv7.
