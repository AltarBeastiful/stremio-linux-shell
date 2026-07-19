#!/usr/bin/env bash
# Unit test for check-nvdec.sh. No Docker, no real libraries: it drives the
# detector against crafted fixture files whose bytes do (or do not) contain the
# markers check-nvdec.sh greps for, and asserts the exit code and output.
#
#   ./packaging/check-nvdec.test.sh
#
# Runs in CI (build.yml) and locally; keep it hermetic (no network, no apt).
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
under_test="$here/check-nvdec.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Fixtures. A real libmpv built with CUDA interop contains the literal string
# "CUDA hwdec"; libavcodec built with ffnvcodec contains "..._nvdec"/"..._cuvid".
printf 'padding CUDA hwdec padding'   > "$tmp/mpv-yes.so"
printf 'padding no interop here'      > "$tmp/mpv-no.so"
printf 'padding hevc_nvdec av1_cuvid' > "$tmp/avc-yes.so"
printf 'padding software only'        > "$tmp/avc-no.so"

fails=0
# run <expected-exit> <label> <LIBMPV_SO> <LIBAVCODEC_SO>
run() {
  local want="$1" label="$2" mpv="$3" avc="$4" out rc
  out="$(LIBMPV_SO="$mpv" LIBAVCODEC_SO="$avc" bash "$under_test" 2>&1)"; rc=$?
  if [ "$rc" -eq "$want" ]; then
    echo "  ok: $label (exit $rc)"
  else
    echo "  FAIL: $label — expected exit $want, got $rc"
    echo "$out" | sed 's/^/      /'
    fails=$((fails + 1))
  fi
}

echo "── check-nvdec.sh unit tests"
run 0 "both halves present"        "$tmp/mpv-yes.so" "$tmp/avc-yes.so"
run 1 "libmpv missing CUDA interop" "$tmp/mpv-no.so"  "$tmp/avc-yes.so"
run 1 "ffmpeg missing ffnvcodec"    "$tmp/mpv-yes.so" "$tmp/avc-no.so"
run 1 "both missing"                "$tmp/mpv-no.so"  "$tmp/avc-no.so"
# exit 2 is "could not locate a library", distinct from 1 ("found it, feature
# absent"). Both are non-zero, so verify-deb.sh treats either as unsatisfied.
run 2 "libmpv path does not exist"  "$tmp/nope.so"    "$tmp/avc-yes.so"
run 2 "ffmpeg path does not exist"  "$tmp/mpv-yes.so" "$tmp/nope.so"

# The machine-readable summary lines other scripts parse must be present.
summary="$(LIBMPV_SO="$tmp/mpv-yes.so" LIBAVCODEC_SO="$tmp/avc-yes.so" bash "$under_test" 2>&1)"
for token in "cuda_interop=YES" "ffnvcodec=YES"; do
  if grep -q "$token" <<<"$summary"; then
    echo "  ok: emits $token"
  else
    echo "  FAIL: missing summary token $token"; fails=$((fails + 1))
  fi
done

echo
if [ "$fails" -ne 0 ]; then echo "check-nvdec.test.sh: FAILED ($fails)"; exit 1; fi
echo "check-nvdec.test.sh: PASSED"
