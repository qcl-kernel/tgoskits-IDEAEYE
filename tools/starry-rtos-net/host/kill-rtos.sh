#!/usr/bin/env bash
# kill-rtos.sh -- kill the FreeRTOS RTOS QEMU guest.
#
# Failure-injection for the "杀掉 RTOS QEMU" test: StarryOS should detect the
# drop, enter reconnect/backoff, and recover once the RTOS is restarted
# (`run-rtos.sh` again).
set -euo pipefail

PIDS="$(pgrep -f 'qemu-system-aarch64.*rtos.bin' || true)"
if [[ -z "$PIDS" ]]; then
    echo "no RTOS QEMU running"
    exit 0
fi
echo "killing RTOS QEMU: $PIDS"
# shellcheck disable=SC2086
kill $PIDS
