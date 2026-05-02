# Updating Programs on a Running Target

Once a target is installed and running a PLC program, you can update the
program without physically accessing the device.

## Recommended: `program/update` (one call)

The agent exposes a single endpoint that picks the best strategy
automatically — online change when the new bundle is layout-compatible
with the running one, otherwise a clean stop-replace-start. The response
tells you which path was taken and how long execution was paused.

```bash
# 1. Rebuild the bundle with your changes
st-cli bundle

# 2. Hand it to the agent — it decides online change vs full restart
curl -X POST -F "file=@MyProject.st-bundle" \
  http://192.168.1.50:4840/api/v1/program/update
```

Example response when the change is layout-compatible:

```json
{
    "success": true,
    "method": "online_change",
    "downtime_ms": 12,
    "program": { "name": "MyProject", "version": "2.0.0", "mode": "development" },
    "online_change": {
        "preserved_vars": ["Main.counter", "Main.flag"],
        "new_vars": [],
        "removed_vars": []
    }
}
```

When the layout changed (added a variable in the middle, changed a type,
etc.) the agent falls back to a full restart, with `method: "restart"`
and the wall-clock downtime in ms.

### From VS Code

The same flow is exposed as the **`Update PLC Program on Target`**
command (palette: search for "update") and as the cloud-upload status
bar button when a target is configured in `plc-project.yaml`. Both
paths build the bundle from the active workspace and POST it to the
agent — the result notification shows the method used and the downtime.

### Manual stop / upload / start (legacy)

If you want explicit control over each phase — for instance to roll
back a botched online change — the underlying endpoints are still
exposed:

```bash
st-cli bundle
curl -X POST http://192.168.1.50:4840/api/v1/program/stop
curl -X POST -F "file=@MyProject.st-bundle" \
  http://192.168.1.50:4840/api/v1/program/upload
curl -X POST http://192.168.1.50:4840/api/v1/program/start
curl -s http://192.168.1.50:4840/api/v1/status | python3 -m json.tool
```

The program info endpoint shows the new version:

```bash
curl -s http://192.168.1.50:4840/api/v1/program/info
```

```json
{
    "name": "MyProject",
    "version": "2.0.0",
    "mode": "development",
    "deployed_at": "2026-04-11T10:30:00Z"
}
```

## Updating While Debugging

If you're debugging remotely from VS Code:

1. **Stop the debug session** in VS Code (Shift+F5)
2. Run the **Update PLC Program on Target** command (or the cloud-upload
   button in the PLC Monitor toolbar)
3. **Re-attach** the debugger — either with the regular F5 launch
   configuration or with the **Live Attach** button (see below)

The debug session starts fresh with the new code. Breakpoints set in the
editor will apply to the updated source.

## Live Attach: Debug Without Stopping the Engine

The PLC Monitor toolbar exposes a **Live Attach** button (next to
Run/Stop, enabled while a program is running). Click it to attach the
VS Code debugger to the running engine *without* halting execution:

- The scan cycle keeps advancing while the debugger is connected.
- Breakpoints fire on demand — the engine pauses only when execution
  reaches a breakpoint line.
- Disconnecting the debugger leaves the engine running.

This is the recommended workflow for inspecting variables on a
production-style target without imposing a scan-time stop. The same
behaviour is reachable from the command palette as
**`Live Attach Debugger to Running Target`**, and from a manual
`launch.json` snippet:

```json
{
    "type": "st",
    "request": "attach",
    "name": "Live Attach to my-plc",
    "target": "my-plc",
    "stopOnEntry": false
}
```

The key flag is `"stopOnEntry": false`. With `stopOnEntry: true` the
agent issues a synthetic Pause on `configurationDone`, which is the
right thing for "debug from the start" but the wrong thing for
"inspect a running production target".

## Runtime Upgrade vs Program Update

These are two different operations:

| Operation | What changes | Command | Downtime |
|-----------|-------------|---------|----------|
| **Program update** | Your ST code | Upload new `.st-bundle` | Seconds (stop + start) |
| **Runtime upgrade** | The `st-runtime` binary | `st-cli target install --upgrade` | Seconds (service restart) |

**Program update** — change your PLC logic, rebuild the bundle, upload. The runtime binary stays the same.

**Runtime upgrade** — a new version of the PLC runtime is available (bug fixes, performance, new features). The `--upgrade` flag preserves your config and deployed programs:

```bash
# Build the new runtime
./scripts/build-static.sh

# Upgrade the target
st-cli target install plc@192.168.1.50 --upgrade
```

If the upgrade fails (new binary crashes), the installer **automatically rolls back** to the previous version:

```
  Backing up current binary...
  Stopping service...
  Uploading new binary...
  Starting service...
  Health check failed — rolling back...
  Restored previous version.
Error: Upgrade failed: agent not healthy after update. Rolled back to previous version.
```

## Monitoring the Update

After updating, check that the program is executing correctly:

```bash
# Check status and cycle stats
curl -s http://192.168.1.50:4840/api/v1/status

# Verify cycle count is advancing
# (run twice with a delay between)
curl -s http://192.168.1.50:4840/api/v1/status | python3 -c "
import sys, json
data = json.load(sys.stdin)
print(f'Status: {data[\"status\"]}')
if data.get('cycle_stats'):
    print(f'Cycles: {data[\"cycle_stats\"][\"cycle_count\"]}')
    print(f'Avg cycle: {data[\"cycle_stats\"][\"avg_cycle_time_us\"]} us')
"
```
