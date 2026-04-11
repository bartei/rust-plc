# Deployment Commands

CLI commands for bundling, target management, and remote deployment.

## `st-cli bundle`

Create a `.st-bundle` archive for deployment.

```bash
st-cli bundle [path] [--release | --release-debug] [-o <output>]
```

| Flag | Description |
|------|-------------|
| `--release` | Strip source files and debug info (IP protection) |
| `--release-debug` | Strip source but keep obfuscated line maps (field diagnostics) |
| `-o`, `--output <path>` | Custom output file path |

**Examples:**
```bash
# Development bundle (default, includes source for debugging)
$ st-cli bundle
Created BottleFillingLine.st-bundle (development, 1.0.0, 5543 bytes)

# Release bundle (no source, stripped variable names)
$ st-cli bundle --release
Created BottleFillingLine.st-bundle (release, 1.0.0, 2850 bytes)

# Custom output path
$ st-cli bundle --release -o dist/program.st-bundle
```

## `st-cli bundle inspect`

Show metadata and contents of a bundle.

```bash
st-cli bundle inspect <bundle-path>
```

**Example:**
```bash
$ st-cli bundle inspect BottleFillingLine.st-bundle

Bundle: BottleFillingLine.st-bundle
  Name:     BottleFillingLine
  Version:  1.0.0
  Mode:     development
  Compiled: 2026-04-10T14:30:00Z
  Compiler: 0.1.1
  Entry:    Main
  Checksum: 166a5025cf03ffbd (valid)
  Size:     5543 bytes

Files:
    281 B  manifest.yaml
    4.6 KB  _io_map.st
    1.1 KB  plc-project.yaml
   86.8 KB  program.stc
    4.6 KB  source/_io_map.st
    1.9 KB  source/main.st
```

## `st-cli target list`

Show deployment targets from `plc-project.yaml`.

```bash
st-cli target list [path]
```

**Example:**
```bash
$ st-cli target list
Deployment targets (plc-project.yaml):
  line1-plc            plc@192.168.1.50:4840 (linux/x86_64) (default)
  line2-plc            plc@192.168.1.51:4840 (linux/aarch64)
```

## `st-cli target install`

Install the PLC runtime on a remote Linux target. One command — everything automated.

```bash
st-cli target install user@host [options]
```

| Flag | Description | Default |
|------|-------------|---------|
| `--key <path>` | SSH private key | Auto-detected |
| `--port <port>` | SSH port | 22 |
| `--agent-port <port>` | Agent HTTP port on target | 4840 |
| `--name <name>` | Agent name | `st-runtime` |
| `--upgrade` | Upgrade existing installation | Fresh install |

**What it does:**
1. Connects via SSH
2. Detects OS and CPU architecture
3. Uploads the static binary (4 MB)
4. Creates directories and writes configuration
5. Installs and starts a systemd service
6. Verifies the agent is healthy

**Examples:**
```bash
# Basic install
$ st-cli target install plc@192.168.1.50

# With explicit key and non-standard SSH port
$ st-cli target install plc@10.0.0.1 --key ~/.ssh/plc_key --port 2222

# Upgrade to new version (preserves config + programs)
$ st-cli target install plc@192.168.1.50 --upgrade
```

## `st-cli target uninstall`

Remove the PLC runtime from a remote target.

```bash
st-cli target uninstall user@host [options]
```

| Flag | Description |
|------|-------------|
| `--purge` | Also remove data directories and logs |
| `--key <path>` | SSH private key |
| `--port <port>` | SSH port |

**Example:**
```bash
# Remove runtime (keeps data and logs)
$ st-cli target uninstall plc@192.168.1.50

# Remove everything
$ st-cli target uninstall plc@192.168.1.50 --purge
```

## VS Code PLC Monitor Toolbar Commands

These commands are accessible from the toolbar in the PLC Monitor panel and from the Command Palette (Ctrl+Shift+P).

| Command | Toolbar Button | Description |
|---------|---------------|-------------|
| `ST: Install PLC Runtime on Target` | ⬇ Install | Install or upgrade the runtime on a target via SSH |
| `ST: Upload PLC Program to Target` | ↑ Upload | Build a bundle and upload to the target (offline) |
| `ST: Online Update PLC Program` | ↻ Online | Build, stop, upload, and restart in one step |
| `ST: Start PLC Program on Target` | ▶ Run | Start the PLC program on the target |
| `ST: Stop PLC Program on Target` | ■ Stop | Stop the PLC program on the target |

The **Install** button opens a VS Code terminal and runs `st-cli target install`. The **Upload** and **Online** buttons also use the terminal for the build+upload sequence. The **Run** and **Stop** buttons call the agent's HTTP API directly for instant feedback.

When multiple targets are configured in `plc-project.yaml`, a quick-pick dropdown lets you select which target to operate on.
