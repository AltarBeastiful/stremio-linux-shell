#!/usr/bin/env bash
# Runtime validation of a built .deb on a REAL NVIDIA box (DEB-BUILD-PLAN step 4).
# The container smoke test (verify-deb.sh TEST 8) proves the archive *ships* an
# nvdec-capable libmpv; this proves it actually decodes zero-copy on this GPU and
# surfaces the WebKitGTK-lag caveat. Part is automatable, part needs a human to
# watch 4K HDR playback — the script does the former and prints the latter.
#
#   sudo ./packaging/nvidia-runtime-check.sh <path-to-.deb | dir-containing-one>
#
# Automated:
#   1. apt-installs the .deb (so libmpv2 + deps resolve like a real user's box)
#   2. runs check-nvdec.sh against the now-installed SYSTEM libmpv/ffmpeg
#   3. reports the system WebKitGTK version + whether the launcher will warn
# Manual (printed as a checklist): play a 4K HDR title and confirm the decode
# mode mpv logs and the CPU of the stremio process, A/B against copy-back.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
arg="${1:?usage: nvidia-runtime-check.sh <.deb or dir>}"

if [ -d "$arg" ]; then
  deb=$(find "$arg" -maxdepth 1 -name '*.deb' | head -1)
else
  deb="$arg"
fi
[ -n "${deb:-}" ] && [ -e "$deb" ] || { echo "no .deb found at: $arg" >&2; exit 1; }

if [ ! -e /dev/nvidia0 ]; then
  echo "WARNING: /dev/nvidia0 not present — this box is not on the NVIDIA"
  echo "proprietary driver, so this script cannot validate the NVIDIA path."
  echo
fi

echo "════ 1. install $(basename "$deb")"
export DEBIAN_FRONTEND=noninteractive
if [ "$(id -u)" -ne 0 ]; then SUDO=sudo; else SUDO=; fi
$SUDO apt-get -qq update >/dev/null 2>&1 || true
$SUDO apt-get -qq install -y "$deb"
echo "   installed."
echo

echo "════ 2. system nvdec capability (this GPU box, not a container)"
if bash "$here/check-nvdec.sh"; then
  echo "   ok — system libmpv/ffmpeg can do nvdec zero-copy."
else
  echo "   PROBLEM — nvdec zero-copy is NOT available on this box; NVIDIA"
  echo "   playback will copy-back (~100% CPU on 4K HDR). Check the distro's"
  echo "   libmpv/ffmpeg build (see packaging/README.md)."
fi
echo

echo "════ 3. system WebKitGTK (B2 UI-lag caveat)"
wk=$(dpkg-query -W -f='${source:Upstream-Version}' libwebkitgtk-6.0-4 2>/dev/null)
echo "   libwebkitgtk-6.0-4: ${wk:-<not installed>}"
if [ -n "$wk" ] && dpkg --compare-versions "$wk" lt 2.52.4; then
  echo "   NOTE — < 2.52.4: the launcher will warn, and the UI is EXPECTED to"
  echo "   lag/artifact on NVIDIA. This is B2, not a regression. To confirm the"
  echo "   fix, A/B against Jeremy Bícha's webkit2gtk PPA (2.52.5) and remove after:"
  echo "     sudo add-apt-repository ppa:webkit-team/ppa && sudo apt upgrade"
else
  echo "   ok — >= 2.52.4 (or unknown): UI lag not expected on this WebKitGTK."
fi
echo

cat <<'EOF'
════ 4. MANUAL — play a 4K HDR title fullscreen and check:

  (a) decode mode — the app sets terminal=yes, so mpv logs the mode. Launch
      from a terminal and watch for the decode line:
        stremio 2>&1 | grep --line-buffered -i 'hardware decoding'
      PASS: "Using hardware decoding (nvdec)"   (zero-copy)
      FAIL: anything ending "-copy" (e.g. vulkan-copy) — copy-back regression.

  (b) CPU — while the 4K HDR title plays fullscreen, in another terminal:
        top -b -n2 -d3 -p "$(pgrep -x stremio | head -1)" | tail -3
      PASS: stremio process ~20-25% of a core.
      A/B: re-run forcing copy-back and confirm it jumps to ~100%:
        STREMIO_HWDEC=auto-copy stremio
      (STREMIO_HWDEC=nvdec forces the zero-copy path back on.)

  (c) UI smoothness — scroll the Stremio UI during playback.
      With WebKitGTK < 2.52.4 on NVIDIA, lag/artifacts are EXPECTED (B2); with
      >= 2.52.4 (or the PPA in step 3) it should be smooth. This is the empirical
      decider for B2 — no WebKit release note names this exact artifact.

Record (a)+(b) per release in DEB-BUILD-PLAN.md; they must pass now (decode is
independent of WebKit). (c) confirms/were it fails confirms the WebKit diagnosis.
EOF
