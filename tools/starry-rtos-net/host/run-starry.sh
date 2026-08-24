#!/usr/bin/env bash
# run-starry.sh -- boot StarryOS in QEMU (x86_64), wired to `br0` via tap0.
#
# Prerequisites:
#   - rootfs present (cargo xtask starry rootfs, or an existing build),
#   - the AXNET/1 client injected into the rootfs (see README.md,
#     "StarryOS 侧"),
#   - setup-bridge.sh run first.
#
# After boot, StarryOS comes up at 192.168.100.2 (statically configured, see
# README.md) and the client connects to 192.168.100.3:5000.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"

exec cargo xtask starry qemu \
    -c os/StarryOS/configs/board/qemu-x86_64.toml \
    --qemu-config tools/starry-rtos-net/host/starry-qemu-tap-x86_64.toml
