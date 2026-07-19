#!/usr/bin/env bash
# Hermetic test for the NVIDIA/WebKitGTK launch-time warning in data/stremio.sh.
# No real GPU and no real WebKitGTK: it stages a fake install prefix (so the
# launcher's exec lands on a no-op stub) and drives the two test hooks the
# launcher honours — STREMIO_NVIDIA_DEV (stand in for /dev/nvidia0) and
# STREMIO_WK_VERSION (stand in for the dpkg-queried WebKitGTK version).
#
#   ./packaging/stremio-launcher.test.sh
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
launcher_src="$here/../data/stremio.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Stage prefix/bin/stremio + prefix/libexec/stremio/stremio (a no-op) so the
# launcher resolves its prefix from $0 and exec's the stub, not the real app.
mkdir -p "$tmp/bin" "$tmp/libexec/stremio"
cp "$launcher_src" "$tmp/bin/stremio"; chmod +x "$tmp/bin/stremio"
printf '#!/bin/sh\nexit 0\n' > "$tmp/libexec/stremio/stremio"
chmod +x "$tmp/libexec/stremio/stremio"

fails=0
# run <should-warn:0|1> <label> <nvidia_dev> <wk_version>
run() {
  local want="$1" label="$2" ndev="$3" wk="$4" err
  err="$(STREMIO_NVIDIA_DEV="$ndev" STREMIO_WK_VERSION="$wk" \
         "$tmp/bin/stremio" 2>&1 >/dev/null)"
  if grep -qi 'webkitgtk' <<<"$err"; then got=1; else got=0; fi
  if [ "$got" -eq "$want" ]; then
    echo "  ok: $label (warn=$got)"
  else
    echo "  FAIL: $label — expected warn=$want, got warn=$got"
    [ -n "$err" ] && echo "$err" | sed 's/^/      /'
    fails=$((fails + 1))
  fi
}

# /dev/null exists, so STREMIO_NVIDIA_DEV=/dev/null simulates "NVIDIA present".
present=/dev/null
absent="$tmp/no-such-nvidia"

echo "── stremio.sh NVIDIA/WebKitGTK warning"
run 1 "NVIDIA + WebKitGTK 2.52.3 (old) warns"     "$present" "2.52.3"
run 0 "NVIDIA + WebKitGTK 2.52.4 (fixed) silent"  "$present" "2.52.4"
run 0 "NVIDIA + WebKitGTK 2.52.5 (newer) silent"  "$present" "2.52.5"
run 0 "NVIDIA + unknown WebKit version silent"    "$present" ""
run 0 "no NVIDIA + old WebKitGTK silent"          "$absent"  "2.52.3"

# The warning must point the user at a remedy (the Flatpak).
msg="$(STREMIO_NVIDIA_DEV=$present STREMIO_WK_VERSION=2.52.3 "$tmp/bin/stremio" 2>&1 >/dev/null)"
if grep -qi 'flatpak' <<<"$msg"; then
  echo "  ok: warning names the Flatpak remedy"
else
  echo "  FAIL: warning does not mention the Flatpak"; fails=$((fails + 1))
fi

echo
if [ "$fails" -ne 0 ]; then echo "stremio-launcher.test.sh: FAILED ($fails)"; exit 1; fi
echo "stremio-launcher.test.sh: PASSED"
