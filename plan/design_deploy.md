# Remote Deployment & Online Management — Design Document

> **Parent plan:** [implementation_core.md](implementation_core.md) — core platform progress tracker (Phases 0-12).
> **Todo list:** [implementation_deploy.md](implementation_deploy.md) — progress tracking and actionable items.
> **See also:**
> - [design_comm.md](design_comm.md) — communication layer design (Phase 13)
> - [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets (Phase 14)

## Overview

A PLC program is useless sitting on the developer's machine. This phase establishes the
infrastructure to **deploy**, **run**, **debug**, **monitor**, and **update** ST programs
on remote target devices — embedded computers running Linux or Windows — over SSH and
network connections.

The primary target is an industrial-grade embedded PC (e.g., Raspberry Pi, BeagleBone,
Advantech IPC, Beckhoff CX series, WAGO PFC, or any x86/ARM Linux or Windows box) that
runs the PLC runtime as a managed service.

---

## Competitive Analysis — What We Take From the Best

| Concept | Inspired By | Our Approach |
|---------|------------|--------------|
| **Gateway agent on target** | CODESYS Runtime | `st-target-agent` — lightweight daemon, manages runtime lifecycle |
| **One-click deploy** | TwinCAT XAE → XAR | `st-cli deploy` or VS Code deploy button |
| **Remote debug** | CODESYS online mode | DAP proxy through the agent — all existing debug features work remotely |
| **Online change** | TwinCAT / CODESYS | Agent invokes online change manager (Phase 9) for hot-reload |
| **Remote monitoring** | TIA Portal online & diagnostics | Monitor WebSocket proxy — existing monitor panel works remotely |
| **Target discovery** | PLCnext scan network | `st-cli target scan` discovers agents on the local network |
| **Project-based target config** | All major IDEs | `targets:` section in `plc-project.yaml` |

**What we do that nobody else does:**
- **SSH-first bootstrap** — no proprietary installer or USB stick. SSH into the target, one
  command deploys the agent. Works with any standard Linux/Windows SSH server.
- **Text-based target config** — target connection details live in `plc-project.yaml`,
  version-controlled alongside the project. No IDE-specific connection database.
- **DAP/Monitor proxy** — existing VS Code debug and monitor features work transparently
  over the network. No separate "remote debug" mode — it's just debug, whether local or remote.
- **Headless-friendly** — full CLI-based workflow for CI/CD pipelines. Deploy, start, stop,
  update, and monitor from scripts without any IDE.

---

## Architecture

```
Developer Machine                              Target Device (Linux/Windows)
┌────────────────────────┐                    ┌──────────────────────────────────┐
│                        │     SSH (bootstrap, │                                  │
│  VS Code Extension     │     tunnel, SCP)    │  st-target-agent                 │
│  ├── Deploy button     │ ◄════════════════► │  ├── HTTP REST API (:4840)       │
│  ├── Target selector   │                    │  │   ├── /api/program/*           │
│  ├── Remote debug      │     TCP/WebSocket  │  │   ├── /api/health              │
│  │   (DAP proxy)       │ ◄────────────────► │  │   └── /api/target-info         │
│  └── Remote monitor    │     (agent port)   │  ├── WebSocket endpoints          │
│      (WS proxy)        │                    │  │   ├── /ws/dap  (DAP proxy)     │
│                        │                    │  │   └── /ws/monitor (telemetry)   │
│  st-cli                │                    │  ├── Runtime Manager               │
│  ├── deploy            │                    │  │   ├── Program store             │
│  ├── target connect    │                    │  │   ├── VM / native process       │
│  ├── target start/stop │                    │  │   ├── Online change (Phase 9)   │
│  ├── target status     │                    │  │   └── Watchdog                  │
│  ├── target update     │                    │  ├── Comm Manager (Phase 13)       │
│  ├── target scan       │                    │  │   └── Device I/O                │
│  └── target logs       │                    │  └── System Integration            │
│                        │                    │      ├── systemd / Windows service  │
│  st-deploy crate       │                    │      ├── Auto-start on boot        │
│  ├── SSH transport     │                    │      └── Crash recovery             │
│  ├── Agent API client  │                    │                                    │
│  └── Program bundler   │                    │  Config: /etc/st-agent/agent.yaml  │
└────────────────────────┘                    │  Programs: /var/lib/st-agent/      │
                                              │  Logs: /var/log/st-agent/          │
                                              └──────────────────────────────────┘
```

---

## Target Agent (`st-target-agent`)

The agent is a standalone Rust binary that runs on the target device as a system service.
It manages the PLC runtime lifecycle and exposes APIs for remote management. It is the
**only** component that needs to be installed on the target.

### Responsibilities

1. **Program storage** — receive and store program bundles (compiled bytecode + source +
   project config + device profiles)
2. **Runtime lifecycle** — start, stop, restart the PLC runtime (VM or native process)
3. **DAP proxy** — spawn a DAP session for the running program and proxy the protocol to
   the remote developer
4. **Monitor proxy** — expose the monitor WebSocket server to the remote developer
5. **Online change** — receive an updated program bundle and apply hot-reload via the
   online change manager (Phase 9)
6. **Health monitoring** — watchdog timer, crash detection, auto-restart on failure
7. **System integration** — systemd unit (Linux) / Windows service for auto-start on boot
8. **Logging** — structured logs to file + journal, queryable via API

### Agent Configuration

```yaml
# /etc/st-agent/agent.yaml
agent:
  name: line1-plc                 # human-readable name (shown in discovery)
  description: "Bottle filling line controller"

network:
  bind: 0.0.0.0                   # listen address
  port: 4840                      # agent API port (HTTP + WebSocket)
  tls:
    enabled: false                # TLS for agent API (recommended for production)
    cert: /etc/st-agent/cert.pem
    key: /etc/st-agent/key.pem

auth:
  mode: token                     # none | token | ssh-key
  token: "changeme"               # shared secret (for token mode)

runtime:
  mode: vm                        # vm | native (Phase 14)
  auto_start: true                # start program on agent boot
  watchdog_ms: 100                # kill and restart if scan cycle exceeds this
  restart_on_crash: true          # auto-restart after unexpected exit
  restart_delay_ms: 1000          # delay before restart
  max_restarts: 5                 # max restarts before giving up (reset on success)

security:
  require_signed: false           # reject unsigned program bundles
  trusted_keys:                   # Ed25519 public keys allowed to sign bundles
    - /etc/st-agent/trusted/deployer.public
  bundle_key: null                # AES-256 key for encrypted bundles (stretch)

storage:
  program_dir: /var/lib/st-agent/programs
  log_dir: /var/log/st-agent
  max_log_size: 50MB
  max_log_files: 10

discovery:
  enabled: true                   # respond to network scan broadcasts
  broadcast_port: 4841            # UDP port for discovery
```

### Program Bundle

A program bundle is a self-contained archive (`.st-bundle`) that contains everything
needed to run the program on the target. Bundles support two modes: **development**
(with source for debugging) and **release** (source-free for IP protection).

#### Bundle Modes

| Mode | Source included | Debug info | Use case |
|------|----------------|------------|----------|
| **development** | Yes (full `.st` files) | Full (line maps, variable names) | Internal development, debugging |
| **release** | No source files | Stripped (no line maps, no var names) | Customer delivery, production |
| **release-debug** | No source files | Obfuscated (line maps only, no source text) | Field support (stack traces without source) |

The mode is selected at bundle creation time:
```bash
st-cli bundle                           # default: development (includes source)
st-cli bundle --release                 # release: no source, stripped debug info
st-cli bundle --release-debug           # release with obfuscated debug info
```

#### IP Protection Design

Protecting proprietary automation code is paramount for industrial deployments. The
release bundle contains **only compiled bytecode** — the original ST source is never
shipped to the target device. This is analogous to shipping a `.exe` without the C
source, or shipping PLC object code in CODESYS/TwinCAT without the project source.

**What is protected:**
- ST source code is **never** included in release bundles
- Variable names in debug info are replaced with opaque indices (`v0`, `v1`, ...)
- POU names can optionally be obfuscated (`--obfuscate-names`)
- The bytecode format is not documented and not trivially reversible
- Bundle integrity is verified by SHA-256 checksum (tampering detection)

**What is NOT protected (by design):**
- Device profile YAML files (needed by the comm manager at runtime)
- Project config `plc-project.yaml` (needed for scan cycle + device setup)
- I/O field names (derived from device profiles, not from user code)

**Defense layers:**

```
Layer 1: No source in bundle     — ST files never leave the developer's machine
Layer 2: Stripped debug info      — no variable names, no line maps in release mode
Layer 3: Bundle signing           — optional Ed25519 signature, agent rejects unsigned
Layer 4: Bundle encryption        — optional AES-256-GCM, agent decrypts with stored key
Layer 5: Bytecode obfuscation     — POU names replaced with hashes (--obfuscate-names)
```

#### Development Bundle Layout

```
my-program.st-bundle (tar.gz)
├── manifest.yaml                 # Bundle metadata
│   ├── name: BottleFillingLine
│   ├── version: 1.2.3
│   ├── mode: development
│   ├── compiled_at: 2026-04-10T14:30:00Z
│   ├── compiler_version: 0.15.0
│   ├── target_arch: x86_64-linux
│   ├── checksum: sha256:abcdef...
│   └── signature: <Ed25519 sig>  # optional
├── program.stc                   # Compiled bytecode (or native binary)
├── debug.map                     # Debug info (line maps, variable names, POU names)
├── source/                       # Original ST source files (for debug)
│   ├── main.st
│   ├── conveyor.st
│   └── fill_controller.st
├── plc-project.yaml              # Project config (cycle_time, devices, etc.)
├── _io_map.st                    # Generated I/O map
└── profiles/                     # Device profiles referenced by the project
    ├── sim_8di_4ai_4do_2ao.yaml
    └── sim_vfd.yaml
```

#### Release Bundle Layout

```
my-program.st-bundle (tar.gz)
├── manifest.yaml                 # Bundle metadata (mode: release)
├── program.stc                   # Compiled bytecode — no source embedded
├── plc-project.yaml              # Runtime config only (cycle_time, devices)
├── _io_map.st                    # Generated I/O map (device field names only)
└── profiles/                     # Device profiles (runtime needs these)
    ├── wago_750_352.yaml
    └── abb_acs580.yaml
```

No `source/` directory. No `debug.map`. The bytecode is all the target needs to execute.

#### Release-Debug Bundle Layout

```
my-program.st-bundle (tar.gz)
├── manifest.yaml                 # Bundle metadata (mode: release-debug)
├── program.stc                   # Compiled bytecode
├── debug.map                     # Obfuscated: line maps (for stack traces),
│                                 #   variable indices (v0, v1...), no source text
├── plc-project.yaml
├── _io_map.st
└── profiles/
```

This enables stack traces with line numbers (for field diagnostics) without exposing
the actual source code. Variable watch during remote debug shows indices (`v0: 42`)
rather than names (`motor_speed: 42`).

#### Bundle Signing

Optional Ed25519 signature for authenticity verification. The developer signs the bundle
with a private key; the agent verifies with a configured public key.

```bash
# Generate a signing key pair
st-cli bundle keygen --output my-key

# Sign a bundle
st-cli bundle --release --sign-key my-key.private

# Configure agent to require signed bundles
# agent.yaml:
#   security:
#     require_signed: true
#     trusted_keys:
#       - /etc/st-agent/trusted/deployer.public
```

The agent rejects unsigned or incorrectly signed bundles when `require_signed: true`.
This prevents unauthorized code from being deployed to production targets.

#### Bundle Encryption (Stretch)

Optional AES-256-GCM encryption for bundles distributed through untrusted channels
(email, USB, file shares). The agent holds the decryption key.

```bash
# Encrypt a bundle for a specific target
st-cli bundle --release --encrypt-for line1-plc

# The target's agent has the decryption key in agent.yaml:
#   security:
#     bundle_key: <base64-encoded AES-256 key>
```

The bundle is created by the compiler and uploaded to the agent. The agent extracts it
to the program store and can start the runtime from it. Release bundles run identically
to development bundles — the runtime only needs the bytecode, not the source.

---

## Transport Layer

### SSH Transport (Bootstrap + Secure Tunnel)

SSH is the universal transport for initial setup and secure operations:

1. **Agent bootstrap** — `st-cli target bootstrap user@host` uploads the agent binary
   via SCP and installs the systemd unit / Windows service
2. **Secure tunnel** — when the agent API is bound to localhost (default secure config),
   the CLI/extension creates an SSH tunnel (`ssh -L local:4840:localhost:4840 user@host`)
   to reach the agent
3. **File transfer** — program bundles uploaded via SCP when the agent API is not available
   (fallback path)

```
Developer                     SSH                      Target
   │                           │                         │
   │  ssh user@host            │                         │
   │ ─────────────────────────►│                         │
   │                           │   scp st-target-agent   │
   │                           │ ───────────────────────►│
   │                           │   systemctl start       │
   │                           │ ───────────────────────►│
   │                           │                         │
   │  ssh -L 4840:localhost:4840 user@host               │
   │ ─────────────────────────►│                         │
   │                           │                         │
   │  HTTP/WS via localhost:4840 (tunneled)              │
   │ ═══════════════════════════════════════════════════►│
```

### Direct Network Transport (Agent API)

When TLS is configured or the network is trusted, the developer connects directly to
the agent's HTTP/WebSocket port without SSH tunneling:

```
Developer                                          Target
   │                                                 │
   │  POST https://192.168.1.50:4840/api/program/upload
   │ ═══════════════════════════════════════════════►│
   │                                                 │
   │  WS wss://192.168.1.50:4840/ws/dap             │
   │ ◄═════════════════════════════════════════════►│
```

---

## Agent REST API

All endpoints prefixed with `/api/v1/`. JSON request/response bodies.

### Program Management

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/v1/program/upload` | Upload program bundle (multipart/form-data) |
| `GET` | `/api/v1/program/info` | Current program metadata (name, version, status) |
| `POST` | `/api/v1/program/start` | Start the PLC runtime |
| `POST` | `/api/v1/program/stop` | Stop the PLC runtime (graceful shutdown) |
| `POST` | `/api/v1/program/restart` | Stop + start |
| `POST` | `/api/v1/program/update` | Upload new bundle + apply online change or restart |
| `DELETE` | `/api/v1/program` | Remove the deployed program |

### Runtime Status & Control

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/status` | Runtime state (running/stopped/error/updating) + cycle stats |
| `GET` | `/api/v1/health` | Agent health check (for load balancers / monitoring) |
| `GET` | `/api/v1/target-info` | Target system info (OS, arch, CPU, RAM, disk) |
| `GET` | `/api/v1/logs` | Query agent + runtime logs (with `?since=` and `?level=` filters) |
| `GET` | `/api/v1/logs/stream` | SSE stream of live log events |

### WebSocket Endpoints

| Endpoint | Description |
|----------|-------------|
| `/ws/dap` | DAP protocol proxy — VS Code connects here for remote debugging |
| `/ws/monitor` | Monitor data stream — VS Code monitor panel connects here |
| `/ws/console` | Runtime console output (print statements, diagnostics) |

### Discovery

| Method | Endpoint | Description |
|--------|----------|-------------|
| UDP broadcast | Port 4841 | Agent responds to `ST-AGENT-DISCOVER` packets with name + port |

---

## Remote Debugging (DAP Proxy)

The agent acts as a DAP proxy. The developer's VS Code connects to the agent's
`/ws/dap` endpoint instead of launching a local DAP process. The agent spawns a
DAP session for the running program and bridges the protocol.

```
VS Code (developer)          Agent                    Runtime (on target)
    │                          │                          │
    │  WS connect /ws/dap      │                          │
    │ ════════════════════════►│                          │
    │                          │  spawn DAP session       │
    │                          │ ────────────────────────►│
    │                          │                          │
    │  Initialize request      │                          │
    │ ────────────────────────►│  Initialize request      │
    │                          │ ────────────────────────►│
    │                          │  Initialize response     │
    │  Initialize response     │ ◄────────────────────────│
    │ ◄────────────────────────│                          │
    │                          │                          │
    │  SetBreakpoints          │  SetBreakpoints          │
    │ ────────────────────────►│ ────────────────────────►│
    │                          │                          │
    │  (all DAP messages proxied bidirectionally)         │
    │                          │                          │
    │  plc/cycleStats event    │  plc/cycleStats event    │
    │ ◄────────────────────────│ ◄────────────────────────│
    │                          │                          │
    │  plc/varCatalog event    │  plc/varCatalog event    │
    │ ◄────────────────────────│ ◄────────────────────────│
```

**Key design decisions:**
- **Transparent proxy** — the agent does not interpret DAP messages, just forwards them.
  This means all existing and future DAP features work without agent changes.
- **Session lifecycle** — the agent starts a DAP session when a developer connects to
  `/ws/dap` and tears it down when they disconnect. Only one debug session at a time.
- **Source mapping** — the source files in the bundle have the same relative paths as
  on the developer's machine. VS Code's `sourceFileMap` launch config can remap paths
  if needed.
- **Attach mode** — the developer attaches to a running program (already started by the
  agent) rather than launching a new one. The DAP session uses `attach` instead of `launch`.

### Debug Capabilities by Bundle Mode

Not all debug features are available for every bundle mode. Release bundles intentionally
strip debug information to protect intellectual property:

| Capability | development | release-debug | release |
|------------|:-----------:|:-------------:|:-------:|
| Breakpoints (by line) | Yes | Yes | No |
| Step In / Over / Out | Yes | Yes | No |
| View source in editor | Yes | No | No |
| Variable names in locals | Yes | No (indices only) | No |
| Variable watch by name | Yes | No | No |
| Stack traces with line numbers | Yes | Yes | No |
| Stack traces with POU names | Yes | Obfuscated | No |
| Force/unforce globals | Yes | By index only | No |
| Cycle stats + monitor | Yes | Yes | Yes |
| Start/stop/restart | Yes | Yes | Yes |

**Development bundles** provide the full debug experience — source stepping, named
variables, everything works as if the program were running locally.

**Release-debug bundles** enable field diagnostics: you can set breakpoints by line
number and see stack traces, but VS Code won't show the source code and variables
appear as `v0`, `v1`, etc. This is sufficient for "the program is stuck at line 47
in POU_a3f2" support tickets without exposing the actual logic.

**Release bundles** are opaque — the runtime executes the bytecode with no debug
hooks. Only monitoring (cycle stats, I/O state) and lifecycle commands (start/stop)
are available. This is the mode for customer-deployed production systems where the
automation code is proprietary.

### VS Code launch.json for Remote Debug

```jsonc
{
    "type": "structured-text",
    "request": "attach",
    "name": "Debug on line1-plc",
    "target": "line1-plc",          // name from plc-project.yaml targets
    // OR direct connection:
    "host": "192.168.1.50",
    "port": 4840,
    "sourceFileMap": {
        "/var/lib/st-agent/programs/current/source": "${workspaceFolder}"
    }
}
```

---

## Remote Monitoring (Monitor Proxy)

The agent exposes the monitor WebSocket on `/ws/monitor`. The VS Code monitor panel
connects to this endpoint and receives telemetry data exactly as if the runtime were
local.

```
VS Code Monitor Panel        Agent                    Runtime
    │                          │                          │
    │  WS connect /ws/monitor  │                          │
    │ ════════════════════════►│                          │
    │                          │  WS connect to local     │
    │                          │  monitor server           │
    │                          │ ════════════════════════►│
    │                          │                          │
    │  addWatch "Main.counter" │  addWatch "Main.counter" │
    │ ────────────────────────►│ ────────────────────────►│
    │                          │                          │
    │  telemetry push          │  telemetry push          │
    │ ◄────────────────────────│ ◄────────────────────────│
```

The monitor proxy is also transparent — it forwards WebSocket frames without
interpretation. Multiple monitor clients can connect simultaneously (the agent
multiplexes to a single connection to the runtime).

---

## Online Update

Online update combines program upload with the online change manager (Phase 9) to
hot-reload a running program without stopping the scan cycle.

### Update Flow

```
Developer                     Agent                    Runtime
    │                          │                          │
    │  POST /program/update    │                          │
    │  (new .st-bundle)        │                          │
    │ ────────────────────────►│                          │
    │                          │  Extract bundle          │
    │                          │  Compare with current    │
    │                          │                          │
    │                          │  [compatible changes?]   │
    │                          │  ┌─ YES ────────────────►│ Online change
    │                          │  │                       │ (hot-reload, Phase 9)
    │                          │  │                       │ Variables migrated
    │                          │  │                       │
    │                          │  └─ NO ─────────────────►│ Stop → Load → Start
    │                          │                          │ (full restart)
    │                          │                          │
    │  200 OK                  │                          │
    │  { "method": "online_change" | "full_restart",      │
    │    "downtime_ms": 0 | 150 }                        │
    │ ◄────────────────────────│                          │
```

**Compatibility check** — the agent compares the old and new program bytecode using the
same logic as the online change manager. If the changes are compatible (same variable
layout, only code changes), hot-reload is applied. Otherwise, a full restart is performed.

The response tells the developer which method was used and the actual downtime.

---

## Target Configuration in plc-project.yaml

```yaml
name: BottleFillingLine
target: host

# ─── Deployment Targets ────────────────────────────────
# Named target devices for deployment. Selected via
# `st-cli deploy --target <name>` or the VS Code target picker.
targets:
  - name: line1-plc
    host: 192.168.1.50
    user: plc
    auth: key                     # key | password | agent (see below)
    os: linux
    arch: x86_64
    agent_port: 4840
    deploy_path: /var/lib/st-agent/programs

  - name: line2-plc
    host: 192.168.1.51
    user: plc
    auth: key
    os: linux
    arch: aarch64                 # ARM64 target — cross-compilation
    agent_port: 4840

  - name: test-bench
    host: 10.0.0.100
    user: admin
    auth: agent                   # direct API (no SSH, requires TLS)
    os: windows
    arch: x86_64
    agent_port: 4840

# Default target (used when --target is omitted)
default_target: line1-plc
```

**Authentication modes:**
- `key` — SSH key authentication (default, required). Uses the developer's SSH agent
  or key file. Password authentication is intentionally not supported — SSH keys
  are more secure, more convenient, and work with automation.
- `agent` — direct agent API connection (no SSH). Requires TLS + token auth on the agent.
  Connection goes directly to `host:agent_port`.

---

## CLI Commands

### Install & Connection

```bash
# Install the PLC runtime on a target device (one command, zero dependencies)
st-cli target install plc@192.168.1.50
st-cli target install plc@192.168.1.50 --key ~/.ssh/plc_key
st-cli target install plc@192.168.1.50 --port 2222  # non-standard SSH port

# Upgrade to a newer version (preserves config + programs)
st-cli target install plc@192.168.1.50 --upgrade

# Uninstall
st-cli target uninstall plc@192.168.1.50

# Verify agent is running and reachable
st-cli target connect line1-plc
st-cli target connect --host 192.168.1.50 --port 4840

# Discover agents on the local network
st-cli target scan
st-cli target scan --subnet 192.168.1.0/24 --timeout 3s
```

### Deployment

```bash
# Deploy the current project to a target
st-cli deploy --target line1-plc
st-cli deploy                              # uses default_target from YAML

# Deploy with explicit bundle
st-cli deploy --target line1-plc --bundle ./my-program.st-bundle

# Build the bundle without deploying
st-cli bundle
st-cli bundle --output ./my-program.st-bundle
```

### Runtime Control

```bash
# Start/stop the program on the target
st-cli target start line1-plc
st-cli target stop line1-plc
st-cli target restart line1-plc

# Check runtime status
st-cli target status line1-plc

# View runtime and agent logs
st-cli target logs line1-plc
st-cli target logs line1-plc --follow --level warn
```

### Online Update

```bash
# Update the running program (auto-selects online change or full restart)
st-cli target update line1-plc
st-cli target update line1-plc --force-restart   # skip online change

# Preview what the update will do (dry run)
st-cli target update line1-plc --dry-run
```

### Target Information

```bash
# List configured targets
st-cli target list

# Show target system info
st-cli target info line1-plc
```

---

## VS Code Extension Integration

### Target Selector

A status bar item showing the currently selected target. Clicking opens a quick-pick
with all configured targets (from `plc-project.yaml`) plus their connection status.

```
┌─────────────────────────────────────────────────────┐
│  Select Deployment Target                            │
│                                                      │
│  ● line1-plc  (192.168.1.50, running)    ← selected │
│  ● line2-plc  (192.168.1.51, stopped)               │
│  ○ test-bench (10.0.0.100, offline)                  │
│  + Add new target...                                 │
└─────────────────────────────────────────────────────┘
```

### Deploy Command

`structured-text.deploy` — builds the program, creates a bundle, uploads to the
selected target, and shows progress in a notification with a progress bar.

### Remote Debug Launch

The extension detects `"request": "attach"` with a `target` field in `launch.json` and
automatically:
1. Resolves the target from `plc-project.yaml`
2. Establishes an SSH tunnel (if auth mode is `key` or `password`)
3. Connects to the agent's `/ws/dap` endpoint
4. Proxies DAP messages between VS Code and the agent

### Remote Monitor

When a remote debug session is active, the monitor panel automatically connects to the
agent's `/ws/monitor` endpoint instead of a local WebSocket server.

### Deploy + Debug Workflow

A compound launch configuration that deploys then debugs in one step:

```jsonc
{
    "type": "structured-text",
    "request": "attach",
    "name": "Deploy & Debug on line1-plc",
    "target": "line1-plc",
    "preLaunchTask": "st-deploy",    // runs st-cli deploy
    "stopOnEntry": true
}
```

---

## Security Model

### Defense in Depth

```
Layer 1: Network          — agent binds to localhost by default; SSH tunnel required
Layer 2: SSH              — key-based authentication for bootstrap and tunneling
Layer 3: TLS              — optional HTTPS/WSS for direct connections
Layer 4: Auth token       — shared secret in agent config, sent in Authorization header
Layer 5: Read-only mode   — agent can be configured to reject program uploads (monitor-only)
```

### SSH Tunnel (Default Secure Mode)

By default, the agent binds to `127.0.0.1:4840`. The developer must establish an SSH
tunnel to reach it. This is the most secure configuration — no ports exposed to the
network, all traffic encrypted and authenticated by SSH.

### Direct TLS Mode

For environments where SSH is not available (Windows targets without OpenSSH, restricted
networks), the agent can be configured with TLS certificates and binds to `0.0.0.0:4840`.
A shared token provides authentication.

### Read-Only Mode

For production targets where you want monitoring but not code changes:

```yaml
# agent.yaml
auth:
  mode: token
  token: "monitor-token"
  read_only: true    # rejects: upload, update, start, stop
                     # allows: status, health, target-info, logs, monitor WS
```

---

## Crate Structure

```
st-deploy/                        # Developer-side deployment logic
├── Cargo.toml                    # depends on: ssh2, reqwest, tokio
└── src/
    ├── lib.rs                    # Public API
    ├── bundle.rs                 # Program bundle creation (.st-bundle)
    ├── ssh.rs                    # SSH connection, SCP upload, tunnel management
    ├── agent_client.rs           # HTTP/WS client for the agent API
    ├── bootstrap.rs              # Agent installation on target
    ├── discovery.rs              # Network scan for agents (UDP broadcast)
    └── target.rs                 # Target config parsing from plc-project.yaml

st-target-agent/                  # Target-side daemon (standalone binary)
├── Cargo.toml                    # depends on: st-engine, st-dap, st-monitor,
│                                 #   axum, tokio-tungstenite, tracing
└── src/
    ├── main.rs                   # Entry point, CLI args, config loading
    ├── config.rs                 # agent.yaml parsing
    ├── server.rs                 # HTTP + WebSocket server (axum)
    ├── api/
    │   ├── program.rs            # /api/v1/program/* handlers
    │   ├── status.rs             # /api/v1/status, /health, /target-info
    │   └── logs.rs               # /api/v1/logs handlers
    ├── proxy/
    │   ├── dap_proxy.rs          # /ws/dap — DAP message forwarding
    │   └── monitor_proxy.rs      # /ws/monitor — telemetry forwarding
    ├── runtime_manager.rs        # Start/stop/restart the PLC runtime
    ├── program_store.rs          # Bundle extraction, storage, versioning
    ├── watchdog.rs               # Crash detection, auto-restart
    ├── discovery.rs              # UDP broadcast responder
    └── service/
        ├── systemd.rs            # systemd unit generation + installation
        └── windows_service.rs    # Windows service registration
```

---

## Static Binary Strategy — Zero Dependencies on Target

**Problem we hit in practice:** A development-build agent linked against glibc 2.39
failed on Debian 12 (glibc 2.36) with `GLIBC_2.39 not found`. Users should never
have to debug library version mismatches on their embedded targets.

**Solution:** The agent ships as a **fully static, self-contained binary** with no
runtime dependencies. The only requirement on the target is a Linux kernel (any
distro, any glibc version, even musl-based Alpine). The user never installs
packages, resolves dependencies, or worries about library compatibility.

### Build Strategy: musl + Static Linking

Both `st-target-agent` and `st-cli` are compiled as fully static ELF binaries
using the `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets.
The tree-sitter C code (the only C dependency) is compiled with `musl-gcc` via
the `CC` environment variable.

```
┌──────────────────────────────────────────────────────────────┐
│ Static Binary Contents                                       │
│ ├── Rust runtime (statically linked)                         │
│ ├── musl libc (statically linked)                            │
│ ├── tree-sitter C library (statically compiled)              │
│ ├── st-engine (VM engine)                                    │
│ ├── st-compiler + st-semantics + st-syntax (compilation)     │
│ ├── st-deploy (bundler)                                      │
│ ├── axum + tokio + hyper (HTTP server)                       │
│ └── All other Rust dependencies                              │
│                                                              │
│ Result: single ELF binary, ~15-25MB stripped                 │
│ Dependencies on target: NONE (not even libc)                 │
└──────────────────────────────────────────────────────────────┘
```

| Target | musl Target Triple | Binary |
|--------|-------------------|--------|
| x86_64 Linux | `x86_64-unknown-linux-musl` | `st-runtime-x86_64-linux` |
| aarch64 Linux | `aarch64-unknown-linux-musl` | `st-runtime-aarch64-linux` |
| armv7 Linux | `armv7-unknown-linux-musleabihf` | `st-runtime-armv7-linux` |

The binary is named `st-runtime` (not `st-target-agent` + `st-cli`). It is a
**single binary** that bundles both the agent and the CLI tool. Subcommands:

```bash
st-runtime agent    # Run as agent daemon (what systemd starts)
st-runtime debug    # DAP debug server (what the agent spawns internally)
st-runtime run      # Direct execution (for quick testing)
st-runtime check    # Syntax/semantic check
st-runtime bundle   # Create deployment bundle
st-runtime version  # Show version info
```

This eliminates the need to deploy two separate binaries (agent + st-cli) and
ensures they are always the same version.

### Build Profiles

```toml
# Cargo.toml profile for release builds
[profile.release-static]
inherits = "release"
opt-level = "s"        # Optimize for size
lto = true             # Link-time optimization
strip = true           # Strip debug symbols
panic = "abort"        # Smaller binary (no unwinding)
codegen-units = 1      # Maximum optimization
```

---

## One-Command Target Installer

**The user experience we want:**

```bash
st-cli target install plc@192.168.1.50
```

That's it. One command. Everything else is automated:

1. SSH into the target (using the user's SSH key)
2. Detect OS and CPU architecture (`uname -s -m`)
3. Select the matching static binary
4. Upload it via SCP (~15-25MB)
5. Create directory structure (`/opt/st-plc/`, `/var/lib/st-plc/`, etc.)
6. Write default `agent.yaml` configuration
7. Generate and install systemd service unit
8. Enable and start the service
9. Wait for the agent to become healthy
10. Report success with connection details

```
$ st-cli target install plc@192.168.1.50

Connecting to plc@192.168.1.50...
  Target: Linux x86_64 (Debian 12)
  Kernel: 6.1.0-44-cloud-amd64

Uploading st-runtime (18.2 MB)...
  ████████████████████████████████████ 100%

Installing...
  Binary:  /opt/st-plc/st-runtime
  Config:  /etc/st-plc/agent.yaml
  Data:    /var/lib/st-plc/
  Logs:    /var/log/st-plc/
  Service: st-runtime.service (systemd)

Starting agent...
  ✓ Agent healthy on port 4840
  ✓ DAP proxy on port 4841

Target 192.168.1.50 is ready.

Add to your project:
  targets:
    - name: my-plc
      host: 192.168.1.50
      user: plc
```

### What the Installer Does on the Target

The installer runs a series of SSH commands on the target. No packages are
installed, no compilers are needed, no libraries are downloaded. The static
binary is the only file that needs to be uploaded.

```bash
# 1. Detect target (runs via SSH)
uname -s -m  # → "Linux x86_64" or "Linux aarch64"

# 2. Create directory structure
sudo mkdir -p /opt/st-plc /etc/st-plc /var/lib/st-plc/programs /var/log/st-plc

# 3. Upload binary (via SCP)
scp st-runtime-x86_64-linux plc@target:/opt/st-plc/st-runtime
sudo chmod +x /opt/st-plc/st-runtime

# 4. Write default config
cat > /etc/st-plc/agent.yaml << 'EOF'
agent:
  name: st-runtime
network:
  bind: 0.0.0.0
  port: 4840
runtime:
  auto_start: true
  restart_on_crash: true
  max_restarts: 10
storage:
  program_dir: /var/lib/st-plc/programs
  log_dir: /var/log/st-plc
EOF

# 5. Install systemd service
cat > /etc/systemd/system/st-runtime.service << 'EOF'
[Unit]
Description=ST PLC Runtime Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
ExecStart=/opt/st-plc/st-runtime agent --config /etc/st-plc/agent.yaml
Restart=on-failure
RestartSec=3
WatchdogSec=30
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# 6. Enable and start
sudo systemctl daemon-reload
sudo systemctl enable st-runtime
sudo systemctl start st-runtime
```

### Upgrade

```bash
st-cli target install plc@192.168.1.50 --upgrade
```

The upgrade command:
1. Stops the service
2. Uploads the new binary (replacing the old one)
3. Restarts the service
4. Verifies the new version is running
5. Preserves the existing configuration and deployed programs

### Uninstall

```bash
st-cli target uninstall plc@192.168.1.50
```

Removes the service, binary, config, and data directories.

### Rollback

If an upgrade fails (agent doesn't start), the installer automatically
restores the previous binary from a backup:

```bash
/opt/st-plc/st-runtime          # current version
/opt/st-plc/st-runtime.backup   # previous version (kept during upgrade)
```

---

## Deployment Workflow (End-to-End)

### First-Time Setup

```
1. Developer runs: st-cli target install plc@192.168.1.50
   → SSH into target (uses developer's SSH key)
   → Detect OS + arch (uname -s -m)
   → Upload static binary (zero dependencies)
   → Create directories, write config
   → Install + start systemd service
   → Verify agent health
   → Print connection details

2. Developer adds target to plc-project.yaml:
   targets:
     - name: line1-plc
       host: 192.168.1.50
       user: plc
```

### Daily Development Cycle

```
1. Developer edits ST code locally
2. LSP provides diagnostics in real-time (local)
3. Developer clicks "Deploy" button (or runs st-cli deploy)
   → Compile project
   → Create .st-bundle
   → Upload to agent via SSH/API
   → Agent extracts and stores the bundle
4. Developer clicks "Start" (or st-cli target start)
   → Agent starts the runtime
5. Developer clicks "Debug" (attach to remote)
   → VS Code connects to /ws/dap on agent
   → Set breakpoints, step, inspect variables
   → Monitor panel connects to /ws/monitor
6. Developer makes a code change
7. Developer clicks "Update" (or st-cli target update)
   → New bundle uploaded
   → Agent applies online change (hot-reload)
   → Debug session continues with new code
```

### CI/CD Pipeline

```bash
#!/bin/bash
# deploy.sh — called by CI after successful build
st-cli bundle --output ./dist/program.st-bundle
st-cli deploy --target production-plc --bundle ./dist/program.st-bundle
st-cli target update production-plc --force-restart
st-cli target status production-plc --wait-running --timeout 30s
```

---

## Error Handling & Recovery

### Agent Crash Recovery

The agent runs as a systemd service with `Restart=on-failure`. If the agent crashes,
systemd restarts it. The agent's startup sequence:
1. Load `agent.yaml`
2. Check for a stored program bundle
3. If `auto_start: true` and a program exists, start the runtime
4. Begin accepting API connections

### Runtime Crash Recovery

If the PLC runtime crashes (VM panic, segfault in native mode):
1. Agent detects the process exit
2. Logs the crash with any available diagnostic info
3. If `restart_on_crash: true`, waits `restart_delay_ms` and restarts
4. If restart count exceeds `max_restarts`, enters error state (stops retrying)
5. Error state is visible via `/api/v1/status` and the VS Code target selector

### Network Interruption

If the developer's debug/monitor connection drops:
- The PLC runtime **keeps running** (the agent is independent)
- The DAP session is torn down on the agent side
- The developer can reconnect at any time
- Variables and breakpoints need to be re-sent on reconnect

---

## End-to-End Testing with QEMU/KVM

The remote deployment pipeline touches SSH, file transfer, systemd services, network
tunnels, WebSocket proxies, and long-running daemons. Unit tests and mock-based
integration tests cannot validate this — we need real VMs running real Linux with
real SSH and real systemd.

### Strategy: Disposable QEMU/KVM Test VMs

Each E2E test suite boots a minimal Linux VM using QEMU/KVM, runs the test battery
against it, and destroys it. This gives us:
- **Real SSH** — key exchange, SCP, tunnel creation over a real TCP connection
- **Real systemd** — service install, enable, start, restart-on-crash, boot persistence
- **Real filesystem** — permissions, disk space, path resolution
- **Real networking** — port forwarding, firewall behavior, connection drops
- **Cross-arch validation** — aarch64 VMs via QEMU emulation (slower, nightly CI)

No mocking means the tests exercise the exact same code paths as a production
deployment to a physical embedded PC.

### VM Architecture

```
CI Runner / Developer Machine
┌─────────────────────────────────────────────────────────┐
│                                                         │
│  Test Runner (cargo test / pytest)                      │
│  ├── vm-manager: start VM, wait for SSH, run tests      │
│  ├── st-cli: deploy, bootstrap, update (real commands)  │
│  └── assertions: query agent API, check state           │
│                                                         │
│      localhost:2222 ──► SSH ──────────┐                  │
│      localhost:4840 ──► Agent API ────┤                  │
│                                       ▼                  │
│  ┌──────────────────────────────────────────┐           │
│  │  QEMU/KVM VM (Debian minimal)            │           │
│  │  ├── openssh-server (:22)                │           │
│  │  ├── systemd (PID 1)                     │           │
│  │  ├── st-target-agent (installed by test) │           │
│  │  └── /var/lib/st-agent/ (program store)  │           │
│  │                                          │           │
│  │  Disk: copy-on-write overlay on base     │           │
│  │  RAM: 512MB                              │           │
│  │  Network: user-mode, port forwarding     │           │
│  └──────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────┘
```

### VM Lifecycle

1. **Base image** — a minimal Debian/Alpine cloud image (~150MB), downloaded once and
   cached. Pre-configured with `openssh-server`, systemd, and a test user.

2. **Copy-on-write overlay** — each test run creates a CoW overlay on the base image
   (`qemu-img create -b base.qcow2 -F qcow2 -f qcow2 test.qcow2`). Tests can trash
   the filesystem without affecting other runs. Overlay is deleted after the test.

3. **Cloud-init** — injects the test runner's SSH public key, sets the hostname, and
   configures the network. No interactive setup needed.

4. **Port forwarding** — QEMU user-mode networking maps `host:2222 → guest:22` (SSH)
   and `host:4840 → guest:4840` (agent API). No bridge setup, no root required.

5. **Snapshot/restore** — after initial boot + SSH ready, take a QEMU snapshot. Each
   test suite restores from the snapshot instead of re-booting (~2s restore vs ~15s boot).

### Test Fixture: ST Test Application

Three versions of the same project for testing different update scenarios:

- **v1** (`test-project/`): counter increments by 1 each cycle, multi-file (main.st +
  helper.st), simulated device I/O, global variables for watch testing.
- **v2** (`test-project-v2/`): same variable layout (online-change compatible), counter
  increments by 2 — proves hot-reload worked by observing the behavior change.
- **v3** (`test-project-v3/`): different variable layout (incompatible) — forces full
  restart, proves the agent handles both paths.

### Test Coverage Matrix

| Test Area | What's Validated |
|-----------|-----------------|
| **Bootstrap** | SSH connect, SCP upload, systemd install, agent starts, reachable |
| **Deploy** | Compile, bundle, upload, extract, program store, start, status |
| **Debug** | DAP attach, breakpoints, stepping, variables, force/unforce, disconnect/reconnect |
| **Monitor** | Watch add/remove, telemetry values, FB tree, multi-client, cycle stats |
| **Online update** | Compatible (hot-reload), incompatible (restart), dry-run, rollback |
| **Bundle modes** | Development (full debug), release (no debug), release-debug (line maps only) |
| **Signing** | Signed bundle accepted, unsigned rejected, tampered rejected |
| **Resilience** | Agent crash → systemd restart, runtime crash → auto-restart, network drop → recovery |
| **Boot persistence** | VM reboot → agent starts → auto-starts last program |

### CI Integration

GitHub Actions `ubuntu-latest` runners support KVM via `/dev/kvm`. The E2E suite runs
as a separate CI job (not blocking the fast unit test job):

```
┌──────────┐     ┌──────────────┐     ┌───────────────┐
│ Unit     │     │ Integration  │     │ E2E Deploy    │
│ Tests    │────►│ Tests        │────►│ (QEMU/KVM)    │
│ (~2 min) │     │ (~3 min)     │     │ (~10 min)     │
└──────────┘     └──────────────┘     └───────────────┘
                                            │
                                      ┌─────▼─────┐
                                      │ ARM64 E2E │
                                      │ (nightly) │
                                      └───────────┘
```

On test failure, the CI job uploads the VM's serial console log + agent logs +
test runner output as artifacts for debugging.

---

## Future Extensions

- **Multi-program targets** — run multiple independent ST programs on one agent (different
  scan rates, different I/O sets)
- **Agent clustering** — coordinate multiple agents for redundant/distributed PLC systems
- **Firmware OTA** — integrate with Phase 14 (native compilation) for firmware updates
  to bare-metal targets
- **Remote file system** — browse and edit ST files on the target directly from VS Code
- **Performance profiling** — per-POU timing data streamed to VS Code (Phase 12 Tier 6)
- **Audit log** — who deployed what and when, stored on the agent
