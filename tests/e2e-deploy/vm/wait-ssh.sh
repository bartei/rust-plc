#!/bin/bash
# Wait until SSH is accepting *authenticated* connections on the given port.
# Usage: ./wait-ssh.sh <port> [timeout_seconds]
#
# First waits for the TCP port to open, then verifies that an actual SSH
# session can be established (cloud-init must have finished injecting keys).

set -euo pipefail

PORT="${1:?Usage: wait-ssh.sh <port> [timeout]}"
TIMEOUT="${2:-90}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
KEY="${SCRIPT_DIR}/images/test_key"

echo -n "Waiting for SSH on port ${PORT}"
for i in $(seq 1 "${TIMEOUT}"); do
    # First check if port is open at all
    if ! nc -z 127.0.0.1 "${PORT}" 2>/dev/null; then
        echo -n "."
        sleep 1
        continue
    fi

    # Port open — now verify SSH actually works (cloud-init may still be running)
    if ssh -o StrictHostKeyChecking=no \
           -o UserKnownHostsFile=/dev/null \
           -o ConnectTimeout=3 \
           -o BatchMode=yes \
           -i "${KEY}" \
           -p "${PORT}" \
           plc@127.0.0.1 "true" 2>/dev/null; then
        echo " ready (${i}s)"
        exit 0
    fi

    echo -n "."
    sleep 1
done

echo " TIMEOUT after ${TIMEOUT}s"
exit 1
