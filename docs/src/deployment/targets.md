# Target Management

A **target** is a remote Linux device that runs the PLC runtime. The runtime is a single 4 MB static binary with zero dependencies — it runs on any Linux distribution regardless of glibc version.

## The Runtime Binary

The `st-plc-runtime` binary is a self-contained executable that includes:

- **Agent** — HTTP API server for deployment and lifecycle management
- **Debugger** — DAP (Debug Adapter Protocol) server for remote debugging from VS Code
- **Compiler** — Full Structured Text compilation pipeline
- **Runtime** — Bytecode VM with scan cycle engine

It is built as a **fully static ELF binary** using musl libc. No shared libraries, no package dependencies, no runtime requirements beyond a Linux kernel.

```
$ file st-plc-runtime
st-plc-runtime: ELF 64-bit LSB pie executable, x86-64, static-pie linked, stripped

$ ldd st-plc-runtime
    statically linked

$ ls -lh st-plc-runtime
-rwxr-xr-x 1 user user 4.0M st-plc-runtime
```

### Building the Static Binary

```bash
./scripts/build-static.sh          # x86_64 (default)
./scripts/build-static.sh aarch64  # ARM64
```

This uses `nix-shell` with the musl cross-compiler toolchain. The output is at:
```
target/x86_64-unknown-linux-musl/release-static/st-plc-runtime
```

## Installing on a Target

```bash
st-cli target install user@host
```

This single command:

1. Connects via SSH (using your existing SSH keys)
2. Detects the target's OS and CPU architecture
3. Uploads the matching static binary (4 MB, takes seconds)
4. Creates the directory structure
5. Writes a default agent configuration
6. Installs and starts a systemd service
7. Verifies the agent is healthy

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `--key <path>` | SSH private key file | Auto-detected from `~/.ssh/` |
| `--port <port>` | SSH port | 22 |
| `--agent-port <port>` | Agent HTTP API port | 4840 |
| `--name <name>` | Agent name (shown in health endpoint) | `st-plc-runtime` |
| `--upgrade` | Upgrade existing installation | Fresh install |

### Upgrading

```bash
st-cli target install user@host --upgrade
```

The upgrade:
- Backs up the current binary
- Stops the service
- Uploads the new binary
- Restarts the service
- Verifies the new version is healthy
- If the new version fails to start, **automatically rolls back** to the backup

Existing configuration and deployed programs are preserved.

### Uninstalling

```bash
# Remove runtime (keeps data and logs)
st-cli target uninstall user@host

# Remove everything
st-cli target uninstall user@host --purge
```

## Target Configuration in `plc-project.yaml`

Define your targets in the project configuration:

```yaml
name: BottleFillingLine
version: "1.0.0"
entryPoint: Main

targets:
  - name: line1-plc
    host: 192.168.1.50
    user: plc
    auth: key
    os: linux
    arch: x86_64
    agent_port: 4840

  - name: line2-plc
    host: 192.168.1.51
    user: plc
    os: linux
    arch: aarch64

  - name: test-bench
    host: 10.0.0.100
    user: admin
    auth: password
    os: windows

default_target: line1-plc
```

### Target Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | Yes | — | Unique identifier for `--target` flag |
| `host` | string | Yes | — | Hostname or IP address |
| `user` | string | No | `plc` | SSH username |
| `auth` | string | No | `key` | Authentication: `key`, `password`, or `agent` |
| `os` | string | No | `linux` | Target OS |
| `arch` | string | No | `x86_64` | CPU architecture |
| `agent_port` | integer | No | 4840 | Agent HTTP API port |
| `deploy_path` | string | No | — | Custom program storage path |

### Listing Targets

```bash
st-cli target list
```

Output:
```
Deployment targets (plc-project.yaml):
  line1-plc            plc@192.168.1.50:4840 (linux/x86_64) (default)
  line2-plc            plc@192.168.1.51:4840 (linux/aarch64)
  test-bench           admin@10.0.0.100:4840 (windows/x86_64)
```

## Systemd Service

The installer creates a systemd service unit at `/etc/systemd/system/st-plc-runtime.service`:

```ini
[Unit]
Description=ST PLC Runtime Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=/opt/st-plc/st-plc-runtime agent --config /etc/st-plc/agent.yaml
Restart=on-failure
RestartSec=3
WatchdogSec=30
StandardOutput=journal
StandardError=journal
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

### Key Properties

| Property | Value | Purpose |
|----------|-------|---------|
| `Restart=on-failure` | Automatic | Restarts the agent if it crashes |
| `RestartSec=3` | 3 seconds | Delay between restart attempts |
| `WatchdogSec=30` | 30 seconds | systemd kills the process if it stops responding |
| `After=network-online.target` | — | Waits for network before starting |
| `WantedBy=multi-user.target` | — | Starts on boot (after `systemctl enable`) |

### Managing the Service

From the target device (via SSH):

```bash
# Check status
sudo systemctl status st-plc-runtime

# View logs
sudo journalctl -u st-plc-runtime -f

# Restart
sudo systemctl restart st-plc-runtime

# Stop
sudo systemctl stop st-plc-runtime

# Disable auto-start
sudo systemctl disable st-plc-runtime
```

## Agent Configuration

The agent reads its configuration from `/etc/st-plc/agent.yaml`:

```yaml
agent:
  name: line1-plc
  description: "Bottle filling line controller"

network:
  bind: 0.0.0.0        # Listen on all interfaces
  port: 4840            # HTTP API port
  # dap_port: 4841      # DAP proxy port (default: port + 1)

runtime:
  auto_start: true      # Start last deployed program on boot
  restart_on_crash: true
  restart_delay_ms: 1000
  max_restarts: 10

storage:
  program_dir: /var/lib/st-plc/programs
  log_dir: /var/log/st-plc

auth:
  mode: none            # none | token
  # token: "my-secret"  # Required when mode: token
  # read_only: false    # Reject uploads/start/stop when true
```

## Agent HTTP API

The agent exposes a REST API on the configured port (default 4840):

### Program Management

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/v1/program/upload` | Upload a `.st-bundle` (multipart form) |
| `GET` | `/api/v1/program/info` | Current program metadata |
| `POST` | `/api/v1/program/start` | Start the PLC runtime |
| `POST` | `/api/v1/program/stop` | Stop the PLC runtime |
| `POST` | `/api/v1/program/restart` | Restart (stop + start) |
| `DELETE` | `/api/v1/program` | Remove the deployed program |

### Status & Monitoring

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/v1/status` | Runtime state + cycle statistics |
| `GET` | `/api/v1/health` | Health check (200 OK / 503) |
| `GET` | `/api/v1/target-info` | OS, arch, agent version, uptime |
| `GET` | `/api/v1/logs` | Query agent log entries |

### DAP Proxy

The agent also listens on a TCP port (default 4841) for DAP debug connections. VS Code connects here when using `request: attach` in `launch.json`. The proxy spawns a debug session for the deployed program and bridges the DAP protocol.

## Directory Layout on Target

After installation, the target has:

```
/opt/st-plc/
  st-plc-runtime              # The runtime binary (4 MB static ELF)

/etc/st-plc/
  agent.yaml                  # Agent configuration

/var/lib/st-plc/
  programs/                   # Deployed program bundles
    current_source/            # Extracted source files (for debugging)

/var/log/st-plc/              # Agent logs

/etc/systemd/system/
  st-plc-runtime.service      # Systemd service unit
```
