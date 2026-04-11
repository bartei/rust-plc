# Troubleshooting

Common issues when deploying and running PLC programs on remote targets.

## Installation Issues

### "Permission denied" during `target install`

```
Error: Permission denied connecting to plc@192.168.1.50. Check your SSH key.
```

**Cause:** The SSH key is not authorized on the target, or the key file is wrong.

**Fix:**
```bash
# Verify you can SSH manually first
ssh plc@192.168.1.50

# If that fails, copy your key to the target
ssh-copy-id plc@192.168.1.50

# Or specify a key explicitly
st-cli target install plc@192.168.1.50 --key ~/.ssh/my_plc_key
```

### "sudo required" error

```
Error: User 'plc' does not have passwordless sudo on 192.168.1.50.
Add to sudoers: plc ALL=(ALL) NOPASSWD:ALL
```

**Cause:** The installer needs sudo to create system directories and install the systemd service.

**Fix:** On the target device:
```bash
echo "plc ALL=(ALL) NOPASSWD:ALL" | sudo tee /etc/sudoers.d/plc
```

### "Connection timeout" or host unreachable

```
Error: SSH connection failed: Connection timed out
```

**Cause:** The target device is not reachable on the network.

**Fix:**
- Verify the IP address: `ping 192.168.1.50`
- Check that SSH is running on the target: `nc -z 192.168.1.50 22`
- If using a non-standard SSH port: `st-cli target install ... --port 2222`
- Check firewall rules on the target

### Binary uploaded but agent doesn't start

**Check the systemd logs:**
```bash
ssh plc@192.168.1.50 "sudo journalctl -u st-plc-runtime --no-pager -n 50"
```

Common causes:
- Port 4840 already in use (another service). Use `--agent-port 5000` during install.
- Configuration error in `/etc/st-plc/agent.yaml`. Check YAML syntax.

## Runtime Issues

### Program upload fails

```json
{"error":"Cannot write temp bundle: ...","code":"internal_error"}
```

**Cause:** Disk full or permission issue on the target.

**Fix:**
```bash
# Check disk space
ssh plc@192.168.1.50 "df -h /var/lib/st-plc/"

# Check permissions
ssh plc@192.168.1.50 "ls -la /var/lib/st-plc/programs/"
```

### "Runtime is already running" when starting

```json
{"error":"Runtime is already running","code":"runtime_already_running"}
```

**Fix:** Stop the current program first:
```bash
curl -X POST http://192.168.1.50:4840/api/v1/program/stop
# Wait a moment
curl -X POST http://192.168.1.50:4840/api/v1/program/start
```

### Program started but cycles aren't advancing

Check the status endpoint:
```bash
curl -s http://192.168.1.50:4840/api/v1/status | python3 -m json.tool
```

If `cycle_count` stays at 0:
- The program may have a compile error. Check the agent logs.
- The entry point PROGRAM may not exist. Verify `entryPoint` in your `plc-project.yaml`.

## Debug Issues

### VS Code debug attach fails to connect

**Check 1:** Is the agent running?
```bash
curl -s http://192.168.1.50:4840/api/v1/health
```

**Check 2:** Is the DAP port accessible?
```bash
nc -z 192.168.1.50 4841 && echo "DAP port open" || echo "DAP port closed"
```

**Check 3:** Is a development bundle deployed? Release bundles reject debug connections.
```bash
curl -s http://192.168.1.50:4840/api/v1/program/info
# Check "mode": should be "development" or "release-debug"
```

**Check 4:** Is the `launch.json` correct?
```jsonc
{
    "type": "st",
    "request": "attach",     // Must be "attach", not "launch"
    "host": "192.168.1.50",  // Agent host
    "port": 4841             // DAP port (agent port + 1)
}
```

### Debug session starts but no source shown

**Cause:** The bundle was created in `release` or `release-debug` mode, which doesn't include source files.

**Fix:** Rebuild with development mode (the default):
```bash
st-cli bundle                 # Development mode (includes source)
# NOT: st-cli bundle --release
```

### Variables show as "v0", "v1" instead of names

**Cause:** The program was deployed with a `release-debug` bundle, which obfuscates variable names.

**Fix:** Use a development bundle for debugging:
```bash
st-cli bundle                 # Includes original variable names
```

## Agent Management

### View agent logs

```bash
# Via SSH
ssh plc@192.168.1.50 "sudo journalctl -u st-plc-runtime -f"

# Most recent entries
ssh plc@192.168.1.50 "sudo journalctl -u st-plc-runtime --no-pager -n 100"

# Errors only
ssh plc@192.168.1.50 "sudo journalctl -u st-plc-runtime -p err --no-pager"
```

### Restart the agent

```bash
ssh plc@192.168.1.50 "sudo systemctl restart st-plc-runtime"
```

### Check agent version

```bash
curl -s http://192.168.1.50:4840/api/v1/target-info | python3 -m json.tool
```

### Reset everything

If you want to start fresh:
```bash
# Uninstall completely
st-cli target uninstall plc@192.168.1.50 --purge

# Reinstall
st-cli target install plc@192.168.1.50
```

## Firewall Ports

The agent uses two TCP ports:

| Port | Protocol | Purpose |
|------|----------|---------|
| 4840 (default) | HTTP | Agent REST API (upload, start, stop, status) |
| 4841 (default) | TCP | DAP debug proxy (VS Code connects here) |

If the target has a firewall, open both ports:

```bash
# UFW (Ubuntu/Debian)
sudo ufw allow 4840/tcp
sudo ufw allow 4841/tcp

# firewalld (RHEL/CentOS)
sudo firewall-cmd --add-port=4840/tcp --permanent
sudo firewall-cmd --add-port=4841/tcp --permanent
sudo firewall-cmd --reload

# iptables
sudo iptables -A INPUT -p tcp --dport 4840 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 4841 -j ACCEPT
```

Or use SSH tunneling to avoid exposing ports entirely (see [Security Configuration](./security.md)).
