#!/bin/bash
# Start a QEMU VM for E2E testing.
# Usage: ./start-vm.sh [x86_64|aarch64]
#
# Port forwarding:
#   x86_64:  host:2222 → guest:22, host:4840 → guest:4840
#   aarch64: host:2223 → guest:22, host:4841 → guest:4840

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE_DIR="${SCRIPT_DIR}/images"

ARCH="${1:-x86_64}"

if [ "${ARCH}" = "x86_64" ]; then
    BASE_IMAGE="${IMAGE_DIR}/debian-x86_64.qcow2"
    OVERLAY="${IMAGE_DIR}/test-x86_64.qcow2"
    PID_FILE="${IMAGE_DIR}/qemu-x86_64.pid"
    SSH_PORT=2222
    AGENT_PORT=4840

    # Create copy-on-write overlay
    qemu-img create -f qcow2 -b "${BASE_IMAGE}" -F qcow2 "${OVERLAY}" 2>/dev/null

    DAP_PORT=$((AGENT_PORT + 1))

    echo "Starting x86_64 VM (SSH: ${SSH_PORT}, Agent: ${AGENT_PORT}, DAP: ${DAP_PORT})..."
    qemu-system-x86_64 \
        -m 1024 -smp 2 \
        -cpu host -enable-kvm \
        -drive file="${OVERLAY}",format=qcow2 \
        -drive file="${IMAGE_DIR}/seed.iso",format=raw \
        -netdev user,id=net0,hostfwd=tcp::${SSH_PORT}-:22,hostfwd=tcp::${AGENT_PORT}-:4840,hostfwd=tcp::${DAP_PORT}-:4841 \
        -device virtio-net-pci,netdev=net0 \
        -display none -daemonize \
        -pidfile "${PID_FILE}" \
        -serial file:"${IMAGE_DIR}/serial-x86_64.log"

elif [ "${ARCH}" = "aarch64" ]; then
    BASE_IMAGE="${IMAGE_DIR}/debian-aarch64.qcow2"
    OVERLAY="${IMAGE_DIR}/test-aarch64.qcow2"
    PID_FILE="${IMAGE_DIR}/qemu-aarch64.pid"
    EFI="${IMAGE_DIR}/QEMU_EFI.fd"
    SSH_PORT=2223
    AGENT_PORT=4841

    if [ ! -f "${EFI}" ]; then
        echo "ERROR: UEFI firmware not found at ${EFI}. Run setup-images.sh first."
        exit 1
    fi

    qemu-img create -f qcow2 -b "${BASE_IMAGE}" -F qcow2 "${OVERLAY}" 2>/dev/null

    DAP_PORT=$((AGENT_PORT + 1))

    echo "Starting aarch64 VM (SSH: ${SSH_PORT}, Agent: ${AGENT_PORT}, DAP: ${DAP_PORT})..."
    qemu-system-aarch64 \
        -M virt -cpu cortex-a72 \
        -m 1024 -smp 2 \
        -bios "${EFI}" \
        -drive file="${OVERLAY}",format=qcow2,if=virtio \
        -drive file="${IMAGE_DIR}/seed.iso",format=raw,if=virtio \
        -netdev user,id=net0,hostfwd=tcp::${SSH_PORT}-:22,hostfwd=tcp::${AGENT_PORT}-:4840,hostfwd=tcp::${DAP_PORT}-:4841 \
        -device virtio-net-pci,netdev=net0 \
        -nographic -daemonize \
        -pidfile "${PID_FILE}" \
        -serial file:"${IMAGE_DIR}/serial-aarch64.log"
else
    echo "Unknown architecture: ${ARCH}. Use x86_64 or aarch64."
    exit 1
fi

echo "VM started (PID: $(cat ${PID_FILE}))"
