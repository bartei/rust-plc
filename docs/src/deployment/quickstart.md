# Deployment Quick Start

This guide walks you through deploying a Structured Text program to a remote target device — from first install to running, debugging, updating, and uninstalling.

**Prerequisites:**
- A Linux target device (Raspberry Pi, industrial PC, embedded box, VM, etc.)
- SSH access to the target with a key-based login
- The user on the target has passwordless `sudo` access

## 1. Install the PLC Runtime on the Target

One command. That's all it takes:

```bash
st-cli target install plc@192.168.1.50
```

This connects via SSH, detects the target's OS and architecture, uploads a 4 MB static binary with zero dependencies, installs a systemd service, and verifies everything is running:

```
Connecting to plc@192.168.1.50...
  Binary: target/x86_64-unknown-linux-musl/release-static/st-plc-runtime (x86_64)
  Connecting...
  Checking sudo access...
  Detecting target platform...
    Target: linux x86_64
  Creating directories...
  Uploading st-plc-runtime (3.9 MB)...
  Verifying binary...
    Version: 0.1.1
  Writing configuration...
  Installing systemd service...
  Starting agent...
  Waiting for agent to become healthy...

Target plc@192.168.1.50 is ready.
  OS:     linux x86_64
  Agent:  port 4840
  DAP:    port 4841
  Version: 0.1.1

Add to your plc-project.yaml:
  targets:
    - name: my-plc
      host: 192.168.1.50
      user: plc
```

**Common options:**

```bash
# Non-standard SSH port
st-cli target install plc@192.168.1.50 --port 2222

# Explicit SSH key
st-cli target install plc@192.168.1.50 --key ~/.ssh/plc_key

# Custom agent name and port
st-cli target install plc@192.168.1.50 --name "line1-plc" --agent-port 5000
```

## 2. Add the Target to Your Project

Add a `targets:` section to your `plc-project.yaml`:

```yaml
name: BottleFillingLine
version: "1.0.0"
entryPoint: Main

engine:
  cycle_time: 10ms

targets:
  - name: line1-plc
    host: 192.168.1.50
    user: plc
```

## 3. Create a Program Bundle

A bundle packages your compiled program for deployment:

```bash
# Development bundle (includes source for debugging)
st-cli bundle

# Release bundle (no source — protects your IP)
st-cli bundle --release
```

Output:
```
Compiling project in /home/user/my-project...
Created BottleFillingLine.st-bundle (development, 1.0.0, 5543 bytes)
```

## 4. The PLC Monitor Toolbar

Open the PLC Monitor panel (**ST: Open PLC Monitor** from the Command Palette or the editor title bar button). The deployment toolbar at the top gives you one-click access to all target operations:

```
┌─ PLC Monitor ──────────────────────────────────────────────────┐
│ [⬇ Install]  │  [↑ Upload] [↻ Online]  │  [▶ Run] [■ Stop]   │
│                                           ● Running — line1-plc│
│────────────────────────────────────────────────────────────────│
│ Scan Cycle                                                      │
│  Cycles: 15,432  Last: 12µs  Avg: 15µs                        │
│────────────────────────────────────────────────────────────────│
│ Watch List                                       [Clear all]    │
│  counter    42    INT                                           │
│  motor_on   TRUE  BOOL                                          │
└────────────────────────────────────────────────────────────────┘
```

| Button | What it does |
|--------|-------------|
| **⬇ Install** | Install or upgrade the PLC runtime on the target device |
| **↑ Upload** | Build a bundle and upload it to the target (offline update — stops the program) |
| **↻ Online** | Build, upload, and restart in one step (online update) |
| **▶ Run** | Start the PLC program on the target |
| **■ Stop** | Stop the PLC program on the target |

The **status indicator** in the top-right shows the target connection state:
- **● Running** (green) — program is executing, cycles advancing
- **● Stopped** (grey) — program is loaded but not running
- **● Error** (red) — runtime error or crash
- **○ No target** (dim) — no target configured

When you click any button, the extension reads the `targets:` section from your `plc-project.yaml` and shows a quick-pick dropdown. If no targets are configured, it prompts for a host.

All toolbar actions are also available from the **Command Palette** (Ctrl+Shift+P):
- `ST: Install PLC Runtime on Target`
- `ST: Upload PLC Program to Target`
- `ST: Online Update PLC Program`
- `ST: Start PLC Program on Target`
- `ST: Stop PLC Program on Target`

## 5. Upload and Run the Program

Click **↑ Upload** in the toolbar (or use the CLI):

```bash
# CLI alternative: build and upload manually
st-cli bundle
curl -X POST -F "file=@BottleFillingLine.st-bundle" \
  http://192.168.1.50:4840/api/v1/program/upload
```

Then click **▶ Run** to start the program. The status indicator turns green and the monitor shows live cycle statistics.

## 6. Debug Remotely from VS Code

Create a `launch.json` in your project with an `attach` configuration:

```jsonc
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "st",
            "request": "attach",
            "name": "Debug on line1-plc",
            "host": "192.168.1.50",
            "port": 4841,
            "stopOnEntry": true
        }
    ]
}
```

Press **F5** to start debugging. VS Code connects to the target's DAP proxy and gives you the full debug experience:

- Set breakpoints by clicking the gutter
- Step In / Step Over / Step Out
- Inspect local and global variables in the Variables panel
- Watch expressions in the Watch panel
- View the call stack
- Evaluate expressions in the Debug Console
- View live cycle statistics in the status bar and PLC Monitor panel

The VS Code debug toolbar (play/pause/step/stop) controls the debug session, while the PLC Monitor toolbar controls the deployment lifecycle. They work together — you can upload a new program, then debug it, all from the same panel.

> **Note:** Remote debugging requires a **development** bundle (the default). Release bundles do not include source or debug info, and the agent will reject debug connections.

## 7. Update the Running Program

Click **↻ Online** in the toolbar to build, upload, and restart in one step.

Or manually:

```bash
st-cli bundle
curl -X POST http://192.168.1.50:4840/api/v1/program/stop
curl -X POST -F "file=@BottleFillingLine.st-bundle" \
  http://192.168.1.50:4840/api/v1/program/upload
curl -X POST http://192.168.1.50:4840/api/v1/program/start
```

## 7. Upgrade the Runtime

When a new version of the PLC runtime is available:

```bash
# Rebuild the static binary
./scripts/build-static.sh

# Upgrade the target (preserves config and deployed programs)
st-cli target install plc@192.168.1.50 --upgrade
```

## 8. Uninstall

```bash
# Remove the runtime (keeps data and logs)
st-cli target uninstall plc@192.168.1.50

# Remove everything including data and logs
st-cli target uninstall plc@192.168.1.50 --purge
```

## Next Steps

- [Bundle Modes & IP Protection](./bundles.md) — development, release, and release-debug bundles
- [Target Management](./targets.md) — configuration, agent API, systemd service
- [Deployment Commands Reference](./commands.md) — all CLI commands for deployment
