#!/bin/bash
# Derive the deb's launcher, D-Bus service and .desktop file from the files the
# Flatpak already ships, so the two packages cannot drift and the shared files
# stay exactly as upstream has them. Only the install prefix (/app -> /usr) and
# the environment the Flatpak manifest would otherwise provide differ.
#
# Output goes to target/deb-stage/, which Cargo.toml's [package.metadata.deb]
# assets read from. Run this before `cargo deb`.
set -euo pipefail

cd "$(dirname "$0")/.."
out=target/deb-stage
mkdir -p "$out"

# The Flatpak manifest sets these as --env finish-args; a deb has no manifest.
sed -e 's#/app/#/usr/#g' \
    -e '1a\
\
export ANV_DEBUG=video-decode,video-encode\
export SERVER_PATH=/usr/libexec/stremio/server.js' \
    data/stremio.sh > "$out/stremio"

sed -e 's#/app/#/usr/#g' data/com.stremio.Stremio.service > "$out/com.stremio.Stremio.service"

# Utility alongside AudioVideo lists the app twice in some menus.
sed -e 's/^Categories=Utility;/Categories=/' data/com.stremio.Stremio.desktop > "$out/com.stremio.Stremio.desktop"
