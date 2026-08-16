#!/usr/bin/env bash
# Build the RT-probe guest and generate an AxVisor VM config pointing at it,
# following the setup_qemu.sh convention (kernel_path patched to an absolute
# path in os/axvisor/tmp/vmconfigs/*.generated.toml).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT/guests/freertos-rt"
./build.sh

ABS_BIN="$(pwd)/freertos-rt.bin"
GEN_DIR="$REPO_ROOT/os/axvisor/tmp/vmconfigs"
mkdir -p "$GEN_DIR"

sed -e "s|^kernel_path = .*|kernel_path = \"$ABS_BIN\"|" \
  "$REPO_ROOT/os/axvisor/configs/vms/qemu/aarch64/freertos-rt-smp1.toml" \
  > "$GEN_DIR/freertos-rt-smp1.generated.toml"

echo "[rt-probe] generated $GEN_DIR/freertos-rt-smp1.generated.toml"
echo "[rt-probe] run axvisor with:"
echo "  cargo xtask axvisor qemu \\"
echo "    --config configs/board/qemu-aarch64-rt.toml \\"
echo "    --qemu-config .github/workflows/qemu-aarch64-rt.toml \\"
echo "    --vmconfigs os/axvisor/tmp/vmconfigs/freertos-rt-smp1.generated.toml"
