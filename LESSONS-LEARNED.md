# Lessons learned — the NVIDIA playback-CPU investigation (2026-07)

The short version: **NVIDIA playback CPU was copy-back video decode, and the fix
is zero-copy `nvdec` (~100% → ~23% on 4K HDR).** Most of the multi-day detour
happened because we tested the **host binary** instead of the **flatpak**, which
behave very differently. These are the lessons so we don't repeat them.

## 1. Test the shipping artifact (the flatpak), not the host binary
The host binary links the **system** GTK/WebKit/graphics libs; the flatpak bundles
`org.gnome.Platform 50` (its own GTK/WebKit/Mesa) and only borrows the host NVIDIA
driver. They are **different graphics environments**. On the host we saw: laggy
idle UI, `gl`-renderer buffer-age artifacts, a "stale overlay" that stuck until a
window remap, black screens under Vulkan, `nvdec` "artifacts". **Every one of
those was host-only.** In the flatpak: none of them reproduce. We burned a full
night building `FreezeOverlay` to fix a "stale overlay" that doesn't exist in the
product. → **Reproduce on the flatpak before believing a bug or a fix.**

## 2. The real NVIDIA cost is copy-back decode, not the overlay
- We assumed the WebKit UI overlay was the cost (it is, on the *host*, where WebKit
  software-renders it as a `GskCairoNode`). In the flatpak the overlay is a cheap
  **Wayland subsurface** — hiding the controls changes CPU by ~1%.
- The ~100% was **`hwdec=auto-copy` copy-back**: decode on the GPU, copy every
  4K/10-bit frame (~25 MB) back through system RAM, re-upload. mpv logs it as
  `Using hardware decoding (vulkan-copy)`.
- Fix: request **`nvdec`** (CUDA-interop **zero-copy**) on NVIDIA. `auto-safe`
  isn't enough — on NVIDIA it only offers copy-back. → ~23%, clean.
- Corollary: **`FreezeOverlay` was the wrong lever** and is abandoned. So was the
  `GSK_RENDERER=gl` idea for NVIDIA — `gl` vs `opengl` is a wash there during real
  playback; the renderer win in PR #108 is Mesa-only.

## 3. Verify a rebuild actually contains your change
`flatpak-builder`'s `dir` source silently reused a **stale cached copy** of the
source: a "successful" rebuild produced a byte-identical artifact (same ostree
commit hash) with none of our change. We then ran several "zero-copy" tests that
were still the old copy-back binary. → After any cached build, **grep the deployed
artifact for a marker** (`strings <bin> | grep STREMIO_HWDEC`) before trusting a
test. If stale: clear `.flatpak-builder/build` + `.flatpak-builder/checksums`
(and any leftover `rofiles` fuse mounts).

## 4. Measure only steady, non-buffering, fullscreen playback
We "confirmed" ~3–4% twice — both were **buffering / stalled** moments with no
decode work. Real steady playback is ~23%. → Take several samples over ~30s during
confirmed smooth fullscreen playback; ignore idle dips. Measure the **`stremio`
GTK process** specifically (`top -p $(pgrep -x stremio)`), not node/WebKit.

## 5. Single-instance flatpak gotcha
`flatpak run` of an already-running app just focuses the existing instance — a
second launch does NOT start your new build. `flatpak kill <app-id>` first (and
verify no `stremio` process remains) before launching a fresh config.

## 6. Downstream packaging (the .deb) depends on libmpv build flags
The NVIDIA win only applies if **libmpv is built with the CUDA interop**
(`--enable-cuda-hwaccel --enable-cuda-interop`, ffmpeg `--enable-ffnvcodec`,
`libplacebo`). Otherwise `nvdec` falls back to copy-back and the ~100% returns.
The flatpak runtime provides this (libmpv 0.41 / ffmpeg 7.1); distro packages must
too. See BENCHMARKS.md.

## What shipped
- `src/app/video/mod.rs` `default_hwdec()`: NVIDIA → `nvdec`, Mesa → `auto-safe`;
  `STREMIO_HWDEC` env override for testing. PR branch `fix/playback-cpu` (`9d8ea17`).
- Docs: CLAUDE.md (goal + rules), BENCHMARKS.md (numbers + repro), this file.
- Abandoned: `FreezeOverlay` (`experiment/nvidia-freeze-overlay`), copy-back-on-NVIDIA.
