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

exec "$prefix/libexec/stremio/stremio" "$@"
