#!/bin/bash
# Wait until SSH is accepting connections on the given port.
# Usage: ./wait-ssh.sh <port> [timeout_seconds]

set -euo pipefail

PORT="${1:?Usage: wait-ssh.sh <port> [timeout]}"
TIMEOUT="${2:-90}"

echo -n "Waiting for SSH on port ${PORT}"
for i in $(seq 1 "${TIMEOUT}"); do
    if nc -z 127.0.0.1 "${PORT}" 2>/dev/null; then
        echo " ready (${i}s)"
        exit 0
    fi
    echo -n "."
    sleep 1
done

echo " TIMEOUT after ${TIMEOUT}s"
exit 1
