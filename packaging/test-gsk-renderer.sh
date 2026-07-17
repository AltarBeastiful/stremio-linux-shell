#!/usr/bin/env bash
# Differential test for the GPU-based GSK renderer decision (src/gpu.rs).
#
# src/gpu.rs decides whether to force GSK_RENDERER=gl based on the GPU. This
# test recomputes that decision independently from the same OS facts, in shell,
# and asserts the real detection code (via the gpu_probe example) agrees. It
# runs on real hardware with no fixtures and no display, so it verifies the CPU
# fix actually activates on whatever machine runs it -- in particular that it
# fires on AMD, not only on Nvidia.
#
# gpu.rs is std-only, so the probe is built with rustc directly: no cargo, no
# GTK/mpv dependency tree, no warm target/ needed.
set -euo pipefail

cd "$(dirname "$0")/.."

# --- reference decision, computed independently from the OS ------------------
# Prefer the GL renderer whenever a GPU is present: the proprietary Nvidia driver
# (/dev/nvidia0) or any primary DRM card (every Mesa driver -- AMD, Intel,
# nouveau). Only a GPU-less software display keeps GTK's default. Keep this in
# step with src/gpu.rs.
expected="gtk-default"
if [ -e /dev/nvidia0 ]; then
    expected="gl"
else
    for card in /sys/class/drm/card[0-9]*; do
        name="$(basename "$card")"
        case "$name" in *-*) continue ;; esac # skip connectors like card1-DP-1
        expected="gl"
        break
    done
fi

# --- actual decision, straight from src/gpu.rs ------------------------------
probe_bin="$(mktemp -d)/gpu_probe"
rustc --edition 2024 -O examples/gpu_probe.rs -o "$probe_bin"
actual="$("$probe_bin")"

echo "expected=$expected actual=$actual"
if [ "$actual" != "$expected" ]; then
    echo "FAIL: gpu_probe disagreed with the reference decision" >&2
    exit 1
fi
echo "PASS"
