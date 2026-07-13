#!/bin/bash

# Use GSK OpenGL renderer for Nvidia cards
if ls /dev/nvidia0 &>/dev/null 2>&1; then
    export GSK_RENDERER=opengl
fi

export SERVER_PATH="/usr/libexec/stremio/server.js"

exec /usr/libexec/stremio/stremio "$@"
