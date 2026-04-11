# Security Configuration

The PLC runtime agent supports multiple security layers to protect deployed programs and control access to the API.

## Authentication Modes

Configure authentication in `/etc/st-plc/agent.yaml`:

### No Authentication (Default)

```yaml
auth:
  mode: none
```

All API requests are accepted without credentials. Suitable for isolated networks or development. The health endpoint (`/api/v1/health`) is always public regardless of auth mode.

### Token Authentication

```yaml
auth:
  mode: token
  token: "my-secret-token-here"
```

Every API request (except `/api/v1/health`) must include the token in the `Authorization` header:

```bash
# With token
curl -H "Authorization: Bearer my-secret-token-here" \
  http://192.168.1.50:4840/api/v1/status

# Without token → 401 Unauthorized
curl http://192.168.1.50:4840/api/v1/status
# → {"error":"Authentication required","code":"auth_required"}

# Wrong token → 403 Forbidden
curl -H "Authorization: Bearer wrong" http://192.168.1.50:4840/api/v1/status
# → {"error":"Invalid authentication token","code":"forbidden"}
```

### Read-Only Mode

```yaml
auth:
  mode: token
  token: "monitor-token"
  read_only: true
```

When `read_only` is enabled, the agent accepts GET requests but rejects all mutating operations (POST, PUT, DELETE):

```bash
# GET works
curl -H "Authorization: Bearer monitor-token" \
  http://192.168.1.50:4840/api/v1/status    # → 200 OK

# POST rejected
curl -X POST -H "Authorization: Bearer monitor-token" \
  http://192.168.1.50:4840/api/v1/program/start
# → {"error":"Agent is in read-only mode","code":"forbidden"}
```

Use this for production monitoring where you want visibility but no risk of accidental program changes.

## SSH Tunnel (Recommended for Remote Access)

By default, the agent binds to `0.0.0.0` (all interfaces). For targets accessible over the internet or untrusted networks, use an SSH tunnel:

```bash
# On the target, bind to localhost only:
# /etc/st-plc/agent.yaml
# network:
#   bind: 127.0.0.1
#   port: 4840

# From your workstation, create a tunnel:
ssh -L 4840:localhost:4840 -L 4841:localhost:4841 plc@192.168.1.50

# Now access the agent via localhost:
curl http://localhost:4840/api/v1/health

# VS Code launch.json for remote debug through tunnel:
# { "type": "st", "request": "attach", "host": "127.0.0.1", "port": 4841 }
```

This way the agent's ports are never exposed to the network — all traffic is encrypted and authenticated by SSH.

## IP Protection via Bundle Modes

The most important security feature for automation vendors is protecting proprietary PLC code from reverse engineering. See [Bundle Modes & IP Protection](./bundles.md) for details.

| Mode | Source Code | Variable Names | Debug Access |
|------|:-----------:|:--------------:|:------------:|
| `development` | Included | Original | Full |
| `release-debug` | Excluded | Obfuscated | Limited |
| `release` | Excluded | Stripped | Rejected |

The agent enforces these protections at runtime — it **rejects debug connections** for release bundles, ensuring that even if someone gains network access to the agent, they cannot inspect the proprietary logic.

## Deployment Security Checklist

For production deployments:

- [ ] Build with `st-cli bundle --release` to strip source and debug info
- [ ] Set `auth.mode: token` with a strong token in `agent.yaml`
- [ ] Consider `auth.read_only: true` if the target should only be monitored
- [ ] Bind to `127.0.0.1` and use SSH tunnels for remote access
- [ ] Restrict SSH access to authorized keys only (no password auth)
- [ ] Use `st-cli target install --upgrade` for updates (preserves config)
