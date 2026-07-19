#!/usr/bin/env bash
# Detect whether the INSTALLED libmpv + ffmpeg can do nvdec zero-copy decode on
# NVIDIA. This is the packaging half of the CPU fix: src/app/video/mod.rs asks
# mpv for hwdec=nvdec on NVIDIA, but nvdec silently falls back to copy-back
# (~100% CPU on 4K HDR) unless BOTH of these independent build-time features are
# present in the distro's libraries:
#
#   (1) libmpv built --enable-cuda-hwaccel --enable-cuda-interop
#         -> the string "CUDA hwdec" is compiled into libmpv.so
#   (2) ffmpeg/libavcodec built --enable-ffnvcodec
#         -> the nvdec/cuvid decoders ("hevc_nvdec", "av1_cuvid", ...) are in
#            libavcodec.so
#
# libmpv links libavcodec dynamically, so (1) says nothing about (2): they must
# be checked separately. grep -a (not `strings`) so this works in a pristine
# container with no binutils — the same reason verify-deb.sh uses grep -a.
#
# Usage:  check-nvdec.sh
# Exit:   0 = both halves present (nvdec zero-copy available)
#         1 = at least one half missing (nvdec would fall back to copy-back)
#         2 = could not locate a library to check
#
# Emits machine-readable summary lines for callers:
#   cuda_interop=YES|NO
#   ffnvcodec=YES|NO
#
# Test hooks: LIBMPV_SO / LIBAVCODEC_SO override library discovery so
# check-nvdec.test.sh can point the detector at fixture files (no real libs,
# no Docker). See packaging/check-nvdec.test.sh.
set -uo pipefail

# ---- locate the libraries (overridable for tests) --------------------------
libmpv="${LIBMPV_SO:-}"
if [ -z "$libmpv" ]; then
  # The versioned SONAME file (libmpv.so.2.x.y), not the -dev symlink.
  libmpv=$(dpkg -L libmpv2 2>/dev/null | grep -E 'libmpv\.so\.2\.[0-9]' | head -1)
fi

avc="${LIBAVCODEC_SO:-}"
if [ -z "$avc" ]; then
  # First glob match is enough; libmpv2 pulls in exactly one libavcodec.
  avc=$(ls /usr/lib/*/libavcodec.so.* 2>/dev/null | head -1)
fi

# ---- (1) libmpv CUDA interop ----------------------------------------------
cuda_interop=NO
if [ -n "$libmpv" ] && [ -e "$libmpv" ]; then
  if grep -a -q 'CUDA hwdec' "$libmpv"; then cuda_interop=YES; fi
else
  echo "check-nvdec: no libmpv.so found (is libmpv2 installed?)" >&2
  echo "cuda_interop=NO"
  echo "ffnvcodec=NO"
  exit 2
fi

# ---- (2) ffmpeg ffnvcodec --------------------------------------------------
ffnvcodec=NO
if [ -n "$avc" ] && [ -e "$avc" ]; then
  if grep -a -q -m1 -E 'nvdec|cuvid' "$avc"; then ffnvcodec=YES; fi
else
  echo "check-nvdec: no libavcodec.so found next to libmpv" >&2
  echo "cuda_interop=$cuda_interop"
  echo "ffnvcodec=NO"
  exit 2
fi

echo "libmpv:     ${libmpv}"
echo "libavcodec: ${avc}"
echo "cuda_interop=$cuda_interop"
echo "ffnvcodec=$ffnvcodec"

[ "$cuda_interop" = YES ] && [ "$ffnvcodec" = YES ]
