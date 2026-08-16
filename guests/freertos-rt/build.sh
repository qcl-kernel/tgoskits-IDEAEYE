#!/usr/bin/env bash
# Build the RT-probe guest into a flat `freertos-rt.bin`.
#
# Requires: cargo (nightly toolchain from repo rust-toolchain.toml) and the
# `rust-objcopy`/`llvm-objcopy` binary (from the rustup `llvm-tools` component,
# which rust-toolchain.toml already lists).
set -euo pipefail
cd "$(dirname "$0")"

cargo build --release

# Locate an objcopy from the llvm-tools component (rust-objcopy or llvm-objcopy).
OBJCOPY="${OBJCOPY:-}"
if [ -z "$OBJCOPY" ]; then
  for c in rust-objcopy llvm-objcopy; do
    if command -v "$c" >/dev/null 2>&1; then OBJCOPY="$c"; break; fi
  done
fi
if [ -z "$OBJCOPY" ]; then
  # rustup llvm-tools installs binaries under lib/rustlib/<target>/bin/.
  for d in "$HOME/.rustup/toolchains"/*/lib/rustlib/*/bin; do
    if [ -x "$d/rust-objcopy" ]; then OBJCOPY="$d/rust-objcopy"; break; fi
    if [ -x "$d/llvm-objcopy" ]; then OBJCOPY="$d/llvm-objcopy"; break; fi
  done
fi
if [ -z "$OBJCOPY" ]; then
  for d in "$HOME/.rustup/toolchains"/*/bin; do
    if [ -x "$d/rust-objcopy" ]; then OBJCOPY="$d/rust-objcopy"; break; fi
    if [ -x "$d/llvm-objcopy" ]; then OBJCOPY="$d/llvm-objcopy"; break; fi
  done
fi
if [ -z "$OBJCOPY" ]; then
  echo "ERROR: no rust-objcopy / llvm-objcopy found (install rustup llvm-tools component)" >&2
  exit 1
fi

"$OBJCOPY" -O binary \
  "target/aarch64-unknown-none-softfloat/release/freertos-rt-probe" \
  freertos-rt.bin

echo "[rt-probe] wrote freertos-rt.bin"
ls -l freertos-rt.bin
