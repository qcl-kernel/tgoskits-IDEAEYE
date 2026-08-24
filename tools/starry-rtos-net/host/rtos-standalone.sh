#!/usr/bin/env bash
# rtos-standalone.sh -- boot the FreeRTOS RTOS guest WITHOUT a bridge, using
# user-mode networking (SLIRP), and forward host port 5000 to the guest.
#
# This is the zero-root validation path.  SLIRP cannot forward to a
# static-IP guest, so this uses the DHCP build (rtos-dhcp.bin, built with
# `make AXNET_DHCP=1`); the guest obtains its address from SLIRP's DHCP.
# While it runs, validate from the host:
#
#   cargo run -p rtos-tester -- --server 127.0.0.1 --port 5000
#   cargo run -p starry-client -- --server 127.0.0.1 --port 5000 --requests 100
#
# The bridged TAP demo (run-rtos.sh) uses the static-IP build (rtos.bin).
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$DIR/rtos/rtos-dhcp.bin"

if [[ ! -f "$BIN" ]]; then
    echo "missing $BIN -- build the DHCP variant first (see rtos/BUILD.md)" >&2
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
    -netdev user,id=net0,hostfwd=tcp::5000-:5000
