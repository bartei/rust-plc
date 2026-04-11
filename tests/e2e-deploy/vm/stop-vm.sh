#!/bin/bash
# Stop a QEMU VM.
# Usage: ./stop-vm.sh [x86_64|aarch64]

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE_DIR="${SCRIPT_DIR}/images"

ARCH="${1:-x86_64}"
PID_FILE="${IMAGE_DIR}/qemu-${ARCH}.pid"
OVERLAY="${IMAGE_DIR}/test-${ARCH}.qcow2"

if [ ! -f "${PID_FILE}" ]; then
    echo "No running ${ARCH} VM found"
    exit 0
fi

PID=$(cat "${PID_FILE}")
echo "Stopping ${ARCH} VM (PID: ${PID})..."

# Try graceful shutdown first
kill -TERM "${PID}" 2>/dev/null || true
for i in $(seq 1 10); do
    if ! kill -0 "${PID}" 2>/dev/null; then
        break
    fi
    sleep 1
done

# Force kill if still running
if kill -0 "${PID}" 2>/dev/null; then
    echo "Force killing..."
    kill -9 "${PID}" 2>/dev/null || true
fi

rm -f "${PID_FILE}" "${OVERLAY}"
echo "VM stopped"
