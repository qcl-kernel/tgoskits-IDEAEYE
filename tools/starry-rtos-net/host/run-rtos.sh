#!/usr/bin/env bash
# run-rtos.sh -- boot the FreeRTOS AXNET/1 RTOS guest in QEMU (AArch64 `virt`).
#
# Connects the guest's virtio-net to `tap1` (see setup-bridge.sh), which
# carries the primary TCP data path to StarryOS.  The guest is configured as
# 192.168.100.3:5000 / MAC 02:00:00:00:01:03.
#
# Usage: run-rtos.sh [tap1]
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$DIR/rtos/rtos.bin"
TAP="${1:-tap1}"

if [[ ! -f "$BIN" ]]; then
    echo "missing $BIN -- build it first (see rtos/BUILD.md)" >&2
    exit 1
fi

exec qemu-system-aarch64 \
    -machine virt,gic-version=2 \
    -global virtio-mmio.force-legacy=false \
    -cpu cortex-a53 \
    -m 256M \
    -smp 1 \
    -display none \
    -serial stdio \
    -monitor none \
    -device loader,file="$BIN",addr=0x40200000 \
    -device virtio-net-device,netdev=net0,mac=02:00:00:00:01:03 \
    -netdev tap,id=net0,ifname="$TAP",script=no,downscript=no
