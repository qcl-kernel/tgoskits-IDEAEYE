#!/usr/bin/env bash
# rtos-standalone.sh -- boot the FreeRTOS RTOS guest WITHOUT a bridge, using
# user-mode networking, and forward host port 5000 to the guest.
#
# This is the zero-root validation path: while it runs, connect from the host
# with the AXNET/1 validator:
#
#   cargo run -p rtos-tester -- --server 127.0.0.1 --port 5000
#
# SLIRP serves the 192.168.100.0/24 subnet so the guest keeps the same
# address (192.168.100.3) it uses in the bridged demo.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$DIR/rtos/rtos.bin"

if [[ ! -f "$BIN" ]]; then
    echo "missing $BIN -- build it first (see rtos/BUILD.md)" >&2
    exit 1
fi

exec qemu-system-aarch64 \
    -machine virt,gic-version=2 \
    -cpu cortex-a53 \
    -m 256M \
    -smp 1 \
    -nographic \
    -device loader,file="$BIN",addr=0x40080000 \
    -device virtio-net-device,netdev=net0,mac=02:00:00:00:01:03 \
    -netdev user,id=net0,net=192.168.100.0/24,dhcpstart=192.168.100.10,hostfwd=tcp::5000-192.168.100.3:5000
