#!/bin/bash

# Resolve the install prefix from this script's own location so the same
# wrapper works in every package: the deb (/usr) and the Flatpak (/app).
self="$0"
case "$self" in */*) ;; *) self="$(command -v -- "$self")" ;; esac
self="$(readlink -f -- "$self")"
prefix="${self%/bin/stremio}"

export LC_NUMERIC=C
export ANV_DEBUG=video-decode,video-encode
export SERVER_PATH="$prefix/libexec/stremio/server.js"

# Use GSK OpenGL renderer for Nvidia cards
if ls /dev/nvidia0 &>/dev/null 2>&1; then
    export GSK_RENDERER=opengl
fi

# NVIDIA + old-WebKitGTK lag notice (deb only; see packaging/README.md and
# DEB-BUILD-PLAN.md B2). WebKitGTK < 2.52.4 lags and shows rendering artifacts
# on the NVIDIA proprietary driver; 2.52.4+ fixes it, and the Flatpak (which
# bundles its own WebKit) is unaffected. This is a soft heads-up, not a hard
# dependency: the .deb must stay installable on every GPU, so it cannot carry a
# global "WebKitGTK >= 2.52.4" Depends while the archive still ships 2.52.3.
# The Flatpak has no dpkg, so dpkg-query returns empty there and this is skipped.
# Test hooks (dry-run on non-NVIDIA hosts): STREMIO_NVIDIA_DEV, STREMIO_WK_VERSION.
nvidia_dev="${STREMIO_NVIDIA_DEV:-/dev/nvidia0}"
# Use ${x-...} (not ${x:-...}) so an explicitly-empty STREMIO_WK_VERSION means
# "treat WebKitGTK as unknown" (the tests' silent path) without invoking dpkg.
wk="${STREMIO_WK_VERSION-$(dpkg-query -W -f='${source:Upstream-Version}' libwebkitgtk-6.0-4 2>/dev/null)}"
if [ -e "$nvidia_dev" ] && [ -n "$wk" ] && dpkg --compare-versions "$wk" lt 2.52.4 2>/dev/null; then
    echo "stremio: WebKitGTK $wk (< 2.52.4) detected on NVIDIA — the UI may lag or show rendering artifacts." >&2
    echo "stremio: this is a known WebKitGTK issue fixed upstream in 2.52.4. Update WebKitGTK, or use the Flatpak (unaffected)." >&2
fi

exec "$prefix/libexec/stremio/stremio" "$@"
