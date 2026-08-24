#!/usr/bin/env bash
# setup-bridge.sh -- create the QEMU L2 network for the StarryOS <-> RTOS demo.
#
#   br0   192.168.100.1/24   Linux bridge
#    |--- tap0  -> StarryOS QEMU  (192.168.100.2)
#    `--- tap1  -> FreeRTOS QEMU  (192.168.100.3)
#
# Requires root.  Idempotent: safe to re-run.
set -euo pipefail

BR="br0"
TAP0="tap0"
TAP1="tap1"
CIDR="192.168.100.1/24"

if ip link show "$BR" >/dev/null 2>&1; then
    echo "bridge $BR already exists"
    exit 0
fi

ip link add "$BR" type bridge
ip link set "$BR" up

ip tuntap add dev "$TAP0" mode tap
ip tuntap add dev "$TAP1" mode tap

ip link set "$TAP0" master "$BR"
ip link set "$TAP1" master "$BR"

ip link set "$TAP0" up
ip link set "$TAP1" up

ip addr add "$CIDR" dev "$BR"

echo "bridge ready:"
bridge link
ip addr show "$BR"
