# CLAUDE.md — project guidance

## ★ TOP PRIORITY GOAL ★
**Implement a fix that lowers playback CPU usage on ALL platforms — Intel, AMD, and NVIDIA.**
- Non-negotiable constraints: **no user disturbance** (no visual artifacts, no stale overlay, no black screens), and it **MUST work at fractional display scale** (the primary dev machine runs 170%).
- This is an **optimization**, not a regression hunt. The target to beat is the *current* CPU, upstream included.

## Status toward the goal
- **AMD / Intel — SOLVED** by upstream PR #108 (`AltarBeastiful:fix/playback-cpu` → `Stremio:main`): `GSK_RENDERER=gl` (avoid the GL→Vulkan copy) + zero-copy hwdec (`auto-safe`, VAAPI dmabuf) + main-loop-sleep fix. KemalK verified on Intel: ~2.5 core → ~0.3 core. Mesa GPU-renders the WebKit UI, so the overlay is free there.
- **NVIDIA — SOLVED** (2026-07-19) by **zero-copy `nvdec`**: 4K HDR **~100% → ~23%** of a core, clean image, validated in the flatpak. Fix is in `default_hwdec()` (`src/app/video/mod.rs`): on `/dev/nvidia0` request `nvdec` (CUDA-interop zero-copy); Mesa keeps `auto-safe`. On the PR branch (`fix/playback-cpu`, commit `9d8ea17`).
  - Root cause was **copy-back decode** (`vulkan-copy`): every 4K/10-bit frame (~25 MB) round-tripped through system RAM. `auto-safe` only offers copy-back on Nvidia, hence the explicit `nvdec`. **NOT the overlay** — the overlay is a cheap Wayland subsurface here (~1%), so `FreezeOverlay` was the WRONG lever and is **abandoned**.
  - The earlier "`nvdec` artifacts → keep copy-back" was a **host-only** libmpv build issue; the flatpak libmpv (built with cuda-interop) is clean.
  - GSK renderer (`gl` vs `opengl`→Vulkan) is a **wash** on Nvidia during real playback (~23% either way).
  - Remaining ~23% = the 4K HDR render/present path (mpv render + GSK + tone-map) — practical floor for this shell. Possible future lever: `GtkGraphicsOffload` on the video (blocked at fractional scale today).
  - **REQUIRES libmpv built with cuda-interop** (`--enable-cuda-hwaccel --enable-cuda-interop`, ffmpeg `--enable-ffnvcodec`) — critical for the .deb, else `nvdec` falls back to copy-back.

## Critical testing rules (learned the hard way — 2026-07-19)
1. **TEST IN THE FLATPAK, not the host binary.** The host binary links system GTK/WebKit and behaves completely differently (laggy, artifacts, stale overlay) from the shipping flatpak, which bundles `org.gnome.Platform 50` and only borrows the host NVIDIA driver. Every "NVIDIA bug" we chased on the host turned out host-only. Build/run the flatpak: `flatpak run com.stremio.Stremio.Devel`.
2. **VERIFY every rebuild picked up your change** before testing: `strings <deployed-binary> | grep <your-new-marker>`. `flatpak-builder`'s `dir` source silently reuses a stale cached copy (same ostree commit hash = stale). If stale, clear `.flatpak-builder/build` + `.flatpak-builder/checksums`.
3. **Never push untested code to the PR / a shared branch.** Run it first.
4. Measure CPU of the **`stremio` GTK process** (not node/WebKit): `top -b -n2 -d 3 -p $(pgrep -x stremio | head -1)`.

## Build / run cheatsheet
- Host dev build (fast, but NOT representative of shipping on NVIDIA): `LC_NUMERIC=C SERVER_PATH=$PWD/data/server.js GSK_RENDERER=gl setsid ./target/release/stremio-linux-shell`. `LC_NUMERIC=C` is mandatory (else mpv_create fails).
- Flatpak build: move `target/` aside first (the `dir` source copies it — 4.7 GB), then `flatpak-builder --user --force-clean --install --repo=flatpak/repo flatpak/build flatpak/com.stremio.Stremio.Devel.json`. Prereqs: `org.gnome.Sdk//50`, `org.freedesktop.Sdk.Extension.rust-stable//25.08`, and `flatpak/cargo-sources.json` (regenerate with `flatpak-cargo-generator.py Cargo.lock`).
- Runtime hwdec override for testing decode modes without a rebuild: `--env=STREMIO_HWDEC=<nvdec|auto-safe|auto-copy|no|...>`.

## Upstream / PR
- Upstream repo: `Stremio/stremio-linux-shell`. Our fork/remote `origin` = `AltarBeastiful/stremio-linux-shell`. PR **#108** is the CPU-fix PR (open). The GitHub PAT is scoped to our own repos — it **cannot** post to the upstream Stremio repo (403); PR comments/description must be pasted by the user.
- Commits & PR posts authored as the user — **no Claude co-author trailer**.
