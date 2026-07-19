#!/usr/bin/env bash
# Build and verify the .deb for each targeted Ubuntu release locally, running
# the same steps CI runs. See packaging/README.md.
#
#   ./packaging/test-deb.sh           # every release in releases.json
#   ./packaging/test-deb.sh noble     # just one
#
# Each release builds in a container of that release, then installs into a
# FRESH container of the same release. The fresh container is the point: it is
# the only way to catch dependencies that are present on the build host but
# missing from the package's Depends.
set -euo pipefail

# Overridable so the script can be run from a copy (and so CI can point it at a
# checkout without caring where the script itself lives).
REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
RELEASES_JSON="$REPO_ROOT/packaging/releases.json"
OUT_DIR="$REPO_ROOT/target/deb-test"

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }

command -v docker >/dev/null || { red "docker is required"; exit 1; }
command -v jq >/dev/null || { red "jq is required"; exit 1; }

only="${1:-}"
mapfile -t rows < <(jq -r '.[] | [.codename, .version, .feature, (.expect_nvdec // false)] | @tsv' "$RELEASES_JSON")

failures=0
tested=0

for row in "${rows[@]}"; do
  IFS=$'\t' read -r codename version feature expect_nvdec <<<"$row"
  [ -n "$only" ] && [ "$only" != "$codename" ] && continue
  tested=$((tested + 1))

  bold "════════════════════════════════════════════════════════"
  bold "  $codename (Ubuntu $version) — feature: $feature"
  bold "════════════════════════════════════════════════════════"

  rm -rf "${OUT_DIR:?}/$codename"
  mkdir -p "$OUT_DIR/$codename"

  # ---- build, in a container of the target release -------------------------
  # Building here (rather than on the host) is what lets cargo-deb's $auto run
  # dpkg-shlibdeps against the libraries this release actually ships.
  echo "── preparing build image (cached after the first run)"
  if ! docker build -q \
    -f "$REPO_ROOT/packaging/Dockerfile.build" \
    --build-arg "CODENAME=$codename" \
    -t "stremio-deb-build:$codename" \
    "$REPO_ROOT/packaging" > "$OUT_DIR/$codename/image.log" 2>&1
  then
    red "  IMAGE BUILD FAILED — last 20 lines:"
    tail -20 "$OUT_DIR/$codename/image.log" | sed 's/^/    /'
    failures=$((failures + 1))
    continue
  fi

  echo "── building .deb in ubuntu:$codename (log: target/deb-test/$codename/build.log)"
  if ! docker run --rm \
    -v "$REPO_ROOT:/src:ro" \
    -v "$OUT_DIR/$codename:/out" \
    -e "CARGO_FEATURE=$feature" \
    -e "UBUNTU_VERSION=$version" \
    "stremio-deb-build:$codename" \
    bash -c 'set -euo pipefail
      # Build out-of-tree: /src is read-only so the host tree stays clean.
      # Exclude target/ and .git/ — copying them is gigabytes of pointless I/O.
      tar -C /src --exclude=./target --exclude=./.git -cf - . | tar -C /build -xf -
      cargo deb --deb-revision "1~ubuntu${UBUNTU_VERSION}" \
        -- --no-default-features --features "${CARGO_FEATURE}"
      cp target/debian/*.deb /out/
    ' > "$OUT_DIR/$codename/build.log" 2>&1
  then
    red "  BUILD FAILED — last 25 lines:"
    tail -25 "$OUT_DIR/$codename/build.log" | sed 's/^/    /'
    failures=$((failures + 1))
    continue
  fi

  deb=$(find "$OUT_DIR/$codename" -maxdepth 1 -name '*.deb' | head -1)
  if [ -z "$deb" ]; then
    red "  BUILD produced no .deb"
    failures=$((failures + 1))
    continue
  fi
  green "  built: $(basename "$deb")"

  # ---- verify, in a FRESH container of the same release --------------------
  echo "── verifying in a fresh ubuntu:$codename (no -dev packages present)"
  # Mount the whole packaging/ dir (read-only) so verify-deb.sh can find its
  # helper check-nvdec.sh next to itself. EXPECT_NVDEC comes from releases.json.
  if docker run --rm \
    -v "$OUT_DIR/$codename:/deb:ro" \
    -v "$REPO_ROOT/packaging:/pkg:ro" \
    -e "EXPECT_NVDEC=$expect_nvdec" \
    ubuntu:"$codename" \
    bash /pkg/verify-deb.sh /deb 2>&1 | sed 's/^/    /'
  then
    green "  $codename PASSED"
  else
    red "  $codename FAILED verification"
    failures=$((failures + 1))
  fi
  echo
done

if [ "$tested" -eq 0 ]; then
  red "no releases matched '${only}' — check packaging/releases.json"
  exit 1
fi
if [ "$failures" -gt 0 ]; then
  red "════ $failures/$tested release(s) FAILED"
  exit 1
fi
green "════ all $tested release(s) passed"
