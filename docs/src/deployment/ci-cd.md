# CI/CD Pipeline

Automate building, bundling, and deploying ST programs to target devices from your CI/CD system.

## Pipeline Overview

```
┌─────────┐    ┌─────────┐    ┌──────────┐    ┌──────────┐    ┌────────┐
│  Build   │───►│  Check  │───►│  Bundle  │───►│  Deploy  │───►│ Verify │
│ st-cli   │    │ st-cli  │    │ st-cli   │    │ curl API │    │ curl   │
│          │    │ check   │    │ bundle   │    │ upload   │    │ status │
└─────────┘    └─────────┘    └──────────┘    └──────────┘    └────────┘
```

## Example: GitHub Actions

```yaml
name: Deploy PLC Program
on:
  push:
    branches: [main]

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Build st-cli
        run: cargo build -p st-cli --release

      - name: Check for errors
        run: ./target/release/st-cli check

      - name: Create release bundle
        run: ./target/release/st-cli bundle --release -o dist/program.st-bundle

      - name: Upload to target
        run: |
          curl -sf -X POST \
            -H "Authorization: Bearer ${{ secrets.PLC_AGENT_TOKEN }}" \
            -F "file=@dist/program.st-bundle" \
            https://plc.internal:4840/api/v1/program/upload

      - name: Stop current program
        run: |
          curl -sf -X POST \
            -H "Authorization: Bearer ${{ secrets.PLC_AGENT_TOKEN }}" \
            https://plc.internal:4840/api/v1/program/stop || true

      - name: Start new program
        run: |
          curl -sf -X POST \
            -H "Authorization: Bearer ${{ secrets.PLC_AGENT_TOKEN }}" \
            https://plc.internal:4840/api/v1/program/start

      - name: Verify running
        run: |
          sleep 2
          STATUS=$(curl -sf \
            -H "Authorization: Bearer ${{ secrets.PLC_AGENT_TOKEN }}" \
            https://plc.internal:4840/api/v1/status | python3 -c "
          import sys, json
          d = json.load(sys.stdin)
          print(d['status'])
          ")
          if [ "$STATUS" != "running" ]; then
            echo "ERROR: Program is not running (status: $STATUS)"
            exit 1
          fi
          echo "Program deployed and running successfully"
```

## Example: Shell Script

A simpler script for manual or cron-based deployment:

```bash
#!/bin/bash
# deploy.sh — Deploy to production PLC
set -euo pipefail

TARGET="plc@192.168.1.50"
AGENT="http://192.168.1.50:4840"
TOKEN="my-secret-token"
AUTH="-H 'Authorization: Bearer ${TOKEN}'"

echo "=== Building bundle ==="
st-cli bundle --release -o /tmp/deploy.st-bundle

echo "=== Stopping current program ==="
curl -sf -X POST ${AUTH} ${AGENT}/api/v1/program/stop || true

echo "=== Uploading new program ==="
curl -sf -X POST ${AUTH} \
  -F "file=@/tmp/deploy.st-bundle" \
  ${AGENT}/api/v1/program/upload

echo "=== Starting program ==="
curl -sf -X POST ${AUTH} ${AGENT}/api/v1/program/start

echo "=== Verifying ==="
sleep 2
curl -sf ${AUTH} ${AGENT}/api/v1/status | python3 -m json.tool

echo "=== Done ==="
rm /tmp/deploy.st-bundle
```

## Key API Endpoints for CI/CD

| Step | Method | Endpoint | Purpose |
|------|--------|----------|---------|
| Upload | `POST` | `/api/v1/program/upload` | Upload `.st-bundle` (multipart) |
| Stop | `POST` | `/api/v1/program/stop` | Stop current program |
| Start | `POST` | `/api/v1/program/start` | Start the new program |
| Verify | `GET` | `/api/v1/status` | Check `status == "running"` |
| Health | `GET` | `/api/v1/health` | Smoke test (200 = agent alive) |
| Info | `GET` | `/api/v1/program/info` | Verify correct version deployed |

## Security for CI/CD

- Use `--release` bundles in production pipelines (no source code in artifact)
- Store the agent token in CI secrets (`${{ secrets.PLC_AGENT_TOKEN }}`)
- Use HTTPS or SSH tunnels for agent access from CI runners
- Consider `auth.read_only: true` on production targets, with a separate deploy token for the CI pipeline
