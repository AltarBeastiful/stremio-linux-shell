#!/usr/bin/env bash
# Verify an already-built .deb, from INSIDE a fresh container of the Ubuntu
# release it was built for.
#
#   ./packaging/verify-deb.sh <dir-containing-deb>
#
# Run by both the local harness (packaging/test-deb.sh) and the smoke-test job
# in .github/workflows/release.yml, so local and CI cannot drift apart.
#
# "Fresh" is the whole point: the build container has every -dev package
# installed, so a .deb with wrong or missing Depends installs fine there and
# fails only on a real user's machine. These checks must run somewhere that has
# never seen a -dev package.
set -euo pipefail

DEB_DIR="${1:?usage: verify-deb.sh <dir-containing-deb>}"
export DEBIAN_FRONTEND=noninteractive

# Files the package must install. apt reports success regardless of whether the
# assets list in Cargo.toml is right, so this is the only thing that catches an
# asset path regressing.
EXPECTED_FILES=(
  /usr/bin/stremio
  /usr/libexec/stremio/stremio
  /usr/libexec/stremio/server.js
  /usr/share/applications/com.stremio.Stremio.desktop
  /usr/share/dbus-1/services/com.stremio.Stremio.service
  /usr/share/metainfo/com.stremio.Stremio.metainfo.xml
  /usr/share/glib-2.0/schemas/com.stremio.Stremio.gschema.xml
  /usr/share/icons/hicolor/scalable/apps/com.stremio.Stremio.svg
)

BIN=/usr/libexec/stremio/stremio
# Whether this target is expected to support nvdec zero-copy on NVIDIA. Set from
# releases.json's per-release "expect_nvdec" flag by the caller (test-deb.sh /
# release.yml). When true, TEST 8 hard-fails a missing capability (regression
# guard); otherwise it only warns. Accept true/1 as truthy.
EXPECT_NVDEC="${EXPECT_NVDEC:-}"
HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
fail=0

deb=$(find "$DEB_DIR" -maxdepth 1 -name '*.deb' | head -1)
[ -n "$deb" ] || { echo "no .deb found in $DEB_DIR"; exit 1; }
echo "verifying: $(basename "$deb")"
echo

echo "── TEST 1: apt resolves every declared dependency"
# The failure this catches: Depends naming packages that do not exist (e.g.
# `libgtk-4` instead of `libgtk-4-1`, which dpkg-shlibdeps invents from the
# SONAME when it cannot resolve one), or a version floor no release satisfies.
apt-get -qq update >/dev/null
if apt-get -qq install -y "$deb" >/tmp/install.log 2>&1; then
  echo "   ok — installed with all dependencies satisfied"
else
  echo "   FAIL — apt could not install the package."
  echo
  echo "   Declared Depends:"
  dpkg-deb -f "$deb" Depends | tr ',' '\n' | sed 's/^ */     /'
  echo
  echo "   Which of those are unsatisfiable here:"
  # apt's own error ("held broken packages") never names the culprit, so check
  # each dependency against what this release actually offers.
  dpkg-deb -f "$deb" Depends | tr ',' '\n' | sed 's/^ *//; s/ *$//' | while read -r dep; do
    [ -z "$dep" ] && continue
    pkg=${dep%% *}
    want=$(sed -n 's/.*(>= \([^)]*\)).*/\1/p' <<<"$dep")
    have=$(apt-cache policy "$pkg" 2>/dev/null | awk '/Candidate:/{print $2}')
    if [ -z "$have" ] || [ "$have" = "(none)" ]; then
      echo "     UNAVAILABLE: $pkg (no such package on this release)"
    elif [ -n "$want" ] && ! dpkg --compare-versions "$have" ge "$want"; then
      echo "     TOO OLD: $pkg wants >= $want, this release has $have"
    fi
  done
  echo
  echo "   apt output:"
  sed 's/^/     /' /tmp/install.log | tail -12
  exit 1
fi
echo

echo "── TEST 2: declared Depends are real, resolvable packages"
deps=$(dpkg-deb -f "$deb" Depends | tr ',' '\n' | sed 's/(.*//; s/^ *//; s/ *$//' | grep -v '^$')
while read -r d; do
  [ -z "$d" ] && continue
  if apt-cache show "$d" >/dev/null 2>&1; then
    echo "   ok: $d"
  else
    echo "   FAIL: '$d' is not a real package"
    fail=1
  fi
done <<<"$deps"
echo

echo "── TEST 3: expected files are installed"
for f in "${EXPECTED_FILES[@]}"; do
  if [ -e "$f" ]; then echo "   ok: $f"; else echo "   FAIL missing: $f"; fail=1; fi
done
echo

echo "── TEST 4: no unresolved shared libraries"
if ldd "$BIN" 2>/dev/null | grep -q "not found"; then
  echo "   FAIL — unresolved:"
  ldd "$BIN" | grep "not found" | sed 's/^/     /'
  fail=1
else
  echo "   ok — every linked library resolves"
fi
echo

echo "── TEST 5: the installed binary executes"
# No display here, so exercise the CLI path clap handles before GTK init.
if out=$("$BIN" --help 2>&1) || out=$("$BIN" --version 2>&1); then
  echo "   ok — binary ran: $(head -1 <<<"$out")"
else
  rc=$?
  echo "   FAIL — binary did not run (exit $rc):"
  sed 's/^/     /' <<<"$out" | head -10
  fail=1
fi
echo

echo "── TEST 6: the GSettings schema was compiled into the cache"
# We ship no postinst: dpkg fires libglib2.0-0's file trigger on
# /usr/share/glib-2.0/schemas and it runs glib-compile-schemas for us. If that
# ever stops happening the app still installs and then fails at runtime the
# moment it touches settings, so assert the end state rather than trusting the
# trigger. grep -a, because the cache is binary and `strings` needs binutils.
CACHE=/usr/share/glib-2.0/schemas/gschemas.compiled
if [ ! -e "$CACHE" ]; then
  echo "   FAIL — $CACHE was never generated (glib trigger did not run)"
  fail=1
elif grep -aq "com.stremio.Stremio" "$CACHE"; then
  echo "   ok — com.stremio.Stremio is in the compiled schema cache"
else
  echo "   FAIL — cache exists but does not contain com.stremio.Stremio"
  fail=1
fi
echo

echo "── TEST 7: the stremio:// URL handler is registered"
# Our .desktop declares MimeType=x-scheme-handler/stremio. Registering it needs
# update-desktop-database from desktop-file-utils, which is not a linked library
# and so cannot be found by $auto — it is a Recommends. apt installs Recommends
# by default, so this also asserts that Recommends line still exists.
MIME=/usr/share/applications/mimeinfo.cache
if [ ! -e "$MIME" ]; then
  echo "   WARN — no mimeinfo.cache; desktop-file-utils absent, stremio:// links will not open the app"
  echo "          (not fatal: the app itself runs. Check the Recommends in Cargo.toml.)"
elif grep -q "x-scheme-handler/stremio" "$MIME"; then
  echo "   ok — x-scheme-handler/stremio maps to $(grep 'x-scheme-handler/stremio' "$MIME" | head -1)"
else
  echo "   WARN — mimeinfo.cache exists but has no stremio:// handler"
fi
echo

echo "── TEST 8: nvdec zero-copy capability (NVIDIA CPU fix)"
# The app requests hwdec=nvdec on NVIDIA (src/app/video/mod.rs). That silently
# degrades to copy-back (~100% CPU on 4K HDR) unless the distro's libmpv has
# CUDA interop AND its libavcodec has ffnvcodec. check-nvdec.sh checks both
# against the libraries this .deb just pulled in. FAIL only when the target is
# flagged expect_nvdec (regression guard) — a blanket fail would block the .deb
# for AMD/Intel users over an NVIDIA-only shortfall (see B2 in DEB-BUILD-PLAN).
case "$EXPECT_NVDEC" in true|True|TRUE|1|yes) expect=1 ;; *) expect=0 ;; esac
checker="$HERE/check-nvdec.sh"
if [ ! -e "$checker" ]; then
  echo "   WARN — check-nvdec.sh not found next to verify-deb.sh; skipping"
  echo "          (mount the packaging/ dir, not just verify-deb.sh)"
elif nv=$(bash "$checker" 2>&1); then
  echo "   ok — nvdec zero-copy available (libmpv CUDA interop + ffmpeg ffnvcodec)"
  sed 's/^/     /' <<<"$nv"
else
  sed 's/^/     /' <<<"$nv"
  if [ "$expect" -eq 1 ]; then
    echo "   FAIL — target is flagged expect_nvdec but nvdec is unavailable;"
    echo "          NVIDIA users would regress to copy-back (~100% CPU on 4K HDR)."
    fail=1
  else
    echo "   WARN — nvdec unavailable on this target; NVIDIA falls back to copy-back."
    echo "          (not flagged expect_nvdec, so not a build failure.)"
  fi
fi
echo

if [ "$fail" -ne 0 ]; then
  echo "RESULT: FAILED"
  exit 1
fi
echo "RESULT: PASSED"
