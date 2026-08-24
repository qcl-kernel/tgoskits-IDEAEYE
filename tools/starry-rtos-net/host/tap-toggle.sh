#!/usr/bin/env bash
# tap-toggle.sh -- cut then restore the RTOS link (network-failure injection).
#
# Usage: tap-toggle.sh [tap1] [delay-seconds]
#
# Brings the RTOS tap down for a few seconds then back up.  The FreeRTOS
# guest does not notice (its virtio link is a device-level concept), but any
# traffic in flight is lost, so StarryOS sees a timeout and recovers via the
# heartbeat/liveness path.
set -euo pipefail

TAP="${1:-tap1}"
DELAY="${2:-5}"

sudo ip link set "$TAP" down
echo "tap $TAP down; restoring in ${DELAY}s"
sleep "$DELAY"
sudo ip link set "$TAP" up
echo "tap $TAP up"
