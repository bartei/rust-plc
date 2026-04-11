# Updating Programs on a Running Target

Once a target is installed and running a PLC program, you can update the program without physically accessing the device.

## Update Workflow

The basic flow: stop the current program, upload the new bundle, start again.

```bash
# 1. Rebuild the bundle with your changes
st-cli bundle

# 2. Stop the running program
curl -X POST http://192.168.1.50:4840/api/v1/program/stop

# 3. Upload the new version
curl -X POST -F "file=@MyProject.st-bundle" \
  http://192.168.1.50:4840/api/v1/program/upload

# 4. Start the new version
curl -X POST http://192.168.1.50:4840/api/v1/program/start

# 5. Verify it's running
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
2. Upload the new bundle (step 2-3 above)
3. **Re-attach** the debugger (F5)

The debug session starts fresh with the new code. Breakpoints set in the editor will apply to the updated source.

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
