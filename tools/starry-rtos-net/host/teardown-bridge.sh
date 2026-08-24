#!/usr/bin/env bash
# teardown-bridge.sh -- remove tap0/tap1 and br0 (see setup-bridge.sh).
set -euo pipefail

ip link del tap0 2>/dev/null || true
ip link del tap1 2>/dev/null || true
ip link del br0 2>/dev/null || true
echo "bridge torn down"
