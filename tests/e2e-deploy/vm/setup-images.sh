#!/bin/bash
# Download cloud images for QEMU E2E tests.
# Usage: ./setup-images.sh [--arch x86_64|aarch64|all]
#
# Downloads minimal cloud images and creates cloud-init seed ISOs.
# Images are cached in ./images/ — only downloaded once.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE_DIR="${SCRIPT_DIR}/images"
CLOUD_INIT_DIR="${SCRIPT_DIR}/cloud-init"

ARCH="${1:-all}"

mkdir -p "${IMAGE_DIR}"

# ── Generate SSH key pair for test runner ────────────────────────────────
KEY_PATH="${IMAGE_DIR}/test_key"
if [ ! -f "${KEY_PATH}" ]; then
    echo "Generating SSH test key pair..."
    ssh-keygen -t ed25519 -f "${KEY_PATH}" -N "" -C "st-agent-e2e-test"
fi
PUBKEY=$(cat "${KEY_PATH}.pub")

# ── Create cloud-init seed ISO ──────────────────────────────────────────
echo "Creating cloud-init seed ISO..."
SEED_DIR=$(mktemp -d)
cp "${CLOUD_INIT_DIR}/meta-data" "${SEED_DIR}/meta-data"
sed "s|__SSH_PUBKEY__|${PUBKEY}|" "${CLOUD_INIT_DIR}/user-data.yaml" > "${SEED_DIR}/user-data"

# Try genisoimage first, then mkisofs
if command -v genisoimage &>/dev/null; then
    genisoimage -output "${IMAGE_DIR}/seed.iso" -volid cidata -joliet -rock \
        "${SEED_DIR}/user-data" "${SEED_DIR}/meta-data" 2>/dev/null
elif command -v mkisofs &>/dev/null; then
    mkisofs -output "${IMAGE_DIR}/seed.iso" -volid cidata -joliet -rock \
        "${SEED_DIR}/user-data" "${SEED_DIR}/meta-data" 2>/dev/null
else
    echo "ERROR: genisoimage or mkisofs required. Install: sudo pacman -S cdrtools"
    exit 1
fi
rm -rf "${SEED_DIR}"
echo "  → ${IMAGE_DIR}/seed.iso"

# ── Download x86_64 image ───────────────────────────────────────────────
if [ "${ARCH}" = "x86_64" ] || [ "${ARCH}" = "all" ]; then
    X86_IMAGE="${IMAGE_DIR}/debian-x86_64.qcow2"
    if [ ! -f "${X86_IMAGE}" ]; then
        echo "Downloading Debian 12 cloud image (x86_64)..."
        curl -L -o "${X86_IMAGE}" \
            "https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2"
        echo "  → ${X86_IMAGE}"
    else
        echo "  x86_64 image already cached"
    fi
fi

# ── Download aarch64 image ──────────────────────────────────────────────
if [ "${ARCH}" = "aarch64" ] || [ "${ARCH}" = "all" ]; then
    ARM_IMAGE="${IMAGE_DIR}/debian-aarch64.qcow2"
    if [ ! -f "${ARM_IMAGE}" ]; then
        echo "Downloading Debian 12 cloud image (aarch64)..."
        curl -L -o "${ARM_IMAGE}" \
            "https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-arm64.qcow2"
        echo "  → ${ARM_IMAGE}"
    else
        echo "  aarch64 image already cached"
    fi

    # Download UEFI firmware for ARM
    EFI_IMAGE="${IMAGE_DIR}/QEMU_EFI.fd"
    if [ ! -f "${EFI_IMAGE}" ]; then
        echo "Downloading UEFI firmware for aarch64..."
        # Try system path first
        for candidate in /usr/share/edk2/aarch64/QEMU_EFI.fd /usr/share/qemu-efi-aarch64/QEMU_EFI.fd /usr/share/OVMF/QEMU_EFI.fd; do
            if [ -f "${candidate}" ]; then
                cp "${candidate}" "${EFI_IMAGE}"
                break
            fi
        done
        if [ ! -f "${EFI_IMAGE}" ]; then
            echo "WARNING: UEFI firmware not found. Install: sudo pacman -S edk2-aarch64"
        fi
    fi
fi

echo "Setup complete."
