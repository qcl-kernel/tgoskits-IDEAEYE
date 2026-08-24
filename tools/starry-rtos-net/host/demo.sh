#!/usr/bin/env bash
# demo.sh -- orchestrate the StarryOS <-> FreeRTOS TCP demo.
#
#   Phase 1  create br0/tap0/tap1,
#   Phase 2  boot the RTOS guest on tap1 (foreground),
#   Phase 3  (separate terminal) run-starry.sh then run the client inside
#            StarryOS; or validate the RTOS server from the host.
#
# Run as root (or with sudo): sudo ./demo.sh
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST="$DIR/host"

if [[ ! -f "$DIR/rtos/rtos.bin" ]]; then
    echo "rtos.bin not found; build it first (see rtos/BUILD.md)" >&2
    exit 1
fi

echo "== Phase 1: create br0 + tap0 + tap1 =="
"$HOST/setup-bridge.sh"

echo "== Phase 2: boot the RTOS guest on tap1 =="
echo "   (Ctrl-A x to quit QEMU)"
echo "   In another terminal you can now:"
echo "     tools/starry-rtos-net/host/run-starry.sh"
echo "   and inside StarryOS:"
echo "     /usr/bin/starry-client --server 192.168.100.3 --port 5000"
exec "$HOST/run-rtos.sh" tap1
