# Playback CPU benchmarks — upstream vs our fix, per platform

Goal: lower playback CPU on **all** platforms. CPU is the `stremio` GTK process
(`top -b -n2 -d 3 -p $(pgrep -x stremio | head -1)`), one core = 100%.
**All numbers are from the flatpak** (the shipping environment; the host binary
is NOT representative — see CLAUDE.md). Measure only during **steady, non-buffering
fullscreen playback** — buffering/idle moments read artificially low (~3%).

| Platform (GPU / driver) | Content | Upstream (stable) | Our fix | Status |
| --- | --- | --- | --- | --- |
| **NVIDIA** GTX 1060 / proprietary 580, @170% | 4K HDR (p010) | **~100%** (copy-back `vulkan-copy`) | **~23%** (`nvdec` zero-copy) | ✅ SOLVED — zero-copy `nvdec`, ~4x, clean image. |
| **Intel** Meteor Lake / Mesa | 1080p H.264 | ~2.5 cores (~250%) | **~0.3 core (~30%)** | ✅ SOLVED by PR #108 (KemalK). |
| **AMD** Radeon 680M / Mesa | 1080p | ~24% (Vulkan renderer) | **~7.8%** (`GSK_RENDERER=gl`) | ✅ SOLVED by PR #108 (probe). |

### NVIDIA breakdown (4K HDR, fullscreen, `nvdec` zero-copy)
| State | `stremio` CPU |
| --- | --- |
| copy-back (`auto-copy`, before) | ~100% |
| **`nvdec` zero-copy, controls hidden (watching)** | **~23%** |
| `nvdec` zero-copy, controls shown | ~24% (overlay adds only ~1%) |

Findings:
- The dominant NVIDIA cost was **copy-back decode** (~75% of the ~100%), not the
  overlay. `nvdec` zero-copy removes it → ~23%.
- The remaining ~23% is the **video render/present path** (mpv render + GSK
  compositing + HDR tone-map at 4K) — roughly the practical floor for this shell.
- **The GSK renderer (`gl` vs `opengl`→Vulkan) is a wash on NVIDIA** during real
  playback (~23% either way) — the renderer win in PR #108 is a Mesa thing.
- **Overlay cost is ~1% here** (controls shown vs hidden). TODO: confirm the
  overlay delta is similarly small on AMD/Intel (cross-platform check).
- Intel/AMD "upstream vs fix" are PR #108's own numbers (different content/method
  than the NVIDIA 4K-HDR runs). Re-run matched 4K-HDR on those laptops to fill in.

## Requirements for the NVIDIA win (important for the .deb)
`nvdec` zero-copy needs **libmpv built with the CUDA interop**, else it silently
falls back to copy-back (~100% again) or software:
- mpv: `--enable-cuda-hwaccel --enable-cuda-interop`
- ffmpeg: `--enable-ffnvcodec` (nvdec), plus `libplacebo`
- Verified with **libmpv 0.41 / ffmpeg 7.1** (org.gnome.Platform 50 runtime).

## How to reproduce (NVIDIA, flatpak)

```sh
# 1. Ensure only one server on :11470 (kill orphans first)
ss -ltnp | grep :11470

# 2. Run the flatpak (default = the fix; nvdec is auto-selected on NVIDIA)
flatpak run com.stremio.Stremio.Devel
#    ...or force a decode mode to compare:
flatpak run --env=STREMIO_HWDEC=nvdec      com.stremio.Stremio.Devel   # zero-copy (fix)
flatpak run --env=STREMIO_HWDEC=auto-copy  com.stremio.Stremio.Devel   # copy-back (before)

# 3. Play a 4K HDR title FULLSCREEN, wait past buffering, then measure the shell:
top -b -n2 -d 3 -p "$(pgrep -x stremio | head -1)" | awk 'END{print $9"%"}'

# 4. Confirm the decode mode actually used (mpv terminal=yes logs it):
#    look for:  Using hardware decoding (nvdec).   VO: [libmpv] 3840x2160 p010
#    (nvdec = zero-copy; nvdec-copy/vulkan-copy = copy-back)
```
