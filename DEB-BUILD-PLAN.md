# Multi-distro .deb build — conclusions + implementation plan

Status of the multi-distro Debian packaging (`packaging/`, `build/deb-multi-distro`)
after the 2026-07 CPU-fix session, and the concrete work left to ship it safely.

## Context: what the .deb is
- `packaging/releases.json` targets Ubuntu **24.04 (noble, `api-4_14`)** and
  **26.04 (resolute, `api-4_22`)** — each pins the GTK/libadwaita/WebKitGTK API
  level of that release via a Cargo feature. Both ship **WebKitGTK 2.52.3**.
- The deb links **system** GTK / WebKitGTK / **libmpv**; the launcher
  `data/stremio.sh` sets `GSK_RENDERER=opengl` on NVIDIA (same as the flatpak).
- CI: `build.yml` **type-checks** every feature set on every push; `release.yml`
  builds the debs (matrix from `releases.json`) + the flatpak on a published release.

## Conclusion
The CPU fix (`nvdec` zero-copy on NVIDIA, `GSK_RENDERER=gl`/VAAPI on Mesa) is
**validated in the flatpak**. Whether it carries to the **.deb** depends on two
things that differ from the flatpak — the **system libmpv build** and the
**system GTK/WebKit build** — because the deb has no bundled runtime. Neither is
guaranteed across distros, and **neither is exercised by CI today** (CI only
compiles). So the deb is *buildable* but **not yet validated at runtime on NVIDIA**.

## Blockers / risks discovered this session

### B1 — `nvdec` needs libmpv built with CUDA interop (the NVIDIA win)
`nvdec` zero-copy silently falls back to copy-back (~100% CPU again) unless libmpv
is built `--enable-cuda-hwaccel --enable-cuda-interop` with ffmpeg
`--enable-ffnvcodec`.
- **Ubuntu 26.04** system `libmpv2` **0.41.0-2ubuntu4 HAS it** — verified
  (`strings libmpv.so` shows `CUDA hwdec`, `bwdif_cuda`; libavcodec has
  `av1_nvdec`/`cuvid`). ✅ deb on 26.04 should get the NVIDIA win.
- **Ubuntu 24.04** — **NOT verified from this machine.** Must confirm its libmpv
  ships the CUDA interop; if not, NVIDIA users on 24.04 get copy-back (no win).
- Any future target release needs the same audit.

### B2 — the deb runs against SYSTEM WebKitGTK = laggy on NVIDIA (ROOT CAUSE FOUND — fixed by a WebKit update)
The deb links **system** GTK/WebKit; the flatpak bundles the GNOME 50 runtime.
Confirmed live: the deb (system libs) on NVIDIA has **laggy UI + WebKit overlay
rendering artifacts + the stale-overlay bugs** — none of which occur in the
flatpak, on the same NVIDIA driver.

**Root cause = a WebKitGTK VERSION gap, not a build-config gap:**
- Ubuntu 26.04 ships WebKitGTK **2.52.3** (`libwebkitgtk-6.0.so.4.16.7`).
- The GNOME 50 flatpak runtime has WebKitGTK **2.52.5** (`.so.4.16.9`) — **two
  micro-releases ahead** (the soname `revision` field: 7=2.52.3, 8=2.52.4, 9=2.52.5).
- GTK is **identical** (4.22.4) in both — not the culprit.
- The fix is the accumulated "Fix several crashes and rendering issues" in
  **WebKitGTK 2.52.4 (2026-06-02) and 2.52.5 (2026-07-09)**. Ubuntu's WebKit build
  flags are fine (`USE_GBM` on, Skia/DMABuf/damage compiled the same) — it's just
  frozen at 2.52.3.

**Resolution:** update system WebKitGTK to **2.52.5** (≥ 2.52.4). This arrives in
26.04 **for free** via Ubuntu's SRU *microrelease exception* (WebKitGTK 2.5x.y
stable releases are pulled into supported releases with the frequent WebKit
security updates). So the deb CAN match the flatpak on NVIDIA — no need to force
NVIDIA users onto the flatpak once WebKit updates.
- **Verify now:** install WebKitGTK 2.52.5 from Jeremy Bícha's `webkit2gtk`
  backport PPA and re-test the deb on NVIDIA.
- NOTE: no WebKit release note names this exact artifact, so verify empirically
  before declaring it closed. Decode (`nvdec`) is independent and works regardless.
- Distinct from the WONTFIX blank-window DMABuf bug (WebKit #262607); that's a
  different failure mode.

### B3 — CI validates compile, not runtime
`build.yml` type-checks each feature set; nothing runs the app or checks NVIDIA
decode/CPU/graphics. `packaging/{test-deb,verify-deb}.sh` check install/deps, not
GPU runtime behaviour. So a distro whose libmpv lacks CUDA interop, or whose
GTK/WebKit lags on NVIDIA, would pass CI and still regress users.

### B4 — WebKitGTK software UI on NVIDIA (shared with flatpak, NOT a blocker)
On NVIDIA proprietary, WebKitGTK software-renders the UI (WONTFIX #262607) in both
deb and flatpak. It's a cheap Wayland subsurface (~1%), so not a cost — noted for
completeness.

## Implementation plan (ordered)

1. **Audit libmpv CUDA interop per target release (B1).** For each entry in
   `releases.json`, confirm the release's `libmpv2` ships CUDA interop:
   `strings $(dpkg -L libmpv2 | grep 'libmpv.so.2.[0-9]') | grep -i 'CUDA hwdec'`.
   26.04 ✅. Do 24.04 (in a 24.04 container/VM). Record results here.
2. **Add a build-time smoke check (B1/B3).** Extend `packaging/verify-deb.sh` to
   assert the installed `libmpv2` has CUDA interop on NVIDIA-capable targets, and
   fail the release leg loudly if a target would silently drop to copy-back.
3. **Per-distro NVIDIA runtime validation (B2/B3).** Build the deb for each
   release, install on an NVIDIA box, play a 4K HDR title fullscreen, and verify:
   (a) mpv logs `Using hardware decoding (nvdec)` (not `*-copy`); (b) `stremio`
   CPU ~20–25% (not ~100%); (c) no laggy UI / rendering artifacts. Use
   `STREMIO_HWDEC=auto-copy` as the A/B baseline. This is the step that decides
   whether B2 is real.
4. **NVIDIA policy (B1/B2).** B2 is a WebKit **version** gap: require/verify
   **WebKitGTK ≥ 2.52.4 (ideally 2.52.5)** for NVIDIA users — it lands in 26.04 for
   free via Ubuntu's SRU microrelease exception. Until it does, note in
   `packaging/README.md` that NVIDIA users on WebKitGTK < 2.52.4 should update
   WebKit (or use the flatpak meanwhile). If a target's libmpv lacks CUDA interop
   (B1), that release can't deliver the NVIDIA decode win — document it.

   **Also — AV1 on pre-Turing NVIDIA:** the GTX 1060 (Pascal) and all pre-Turing
   NVIDIA GPUs have **no AV1 hardware decoder**. AV1 content falls back to software
   decode (heavy at 4K) regardless of `nvdec`/deb/flatpak — a GPU hardware limit,
   not a bug. `nvdec` zero-copy applies to H.264/HEVC (Pascal-supported). Worth a
   line in `packaging/README.md` so AV1-on-old-NVIDIA lag isn't mistaken for a bug.
5. **Document requirements.** In `packaging/README.md`: the libmpv CUDA-interop
   requirement, the `STREMIO_HWDEC` override for support/debugging, and the
   deb-vs-flatpak caveat for NVIDIA.
6. **(Optional) extend CI.** A NVIDIA-tagged self-hosted runner (or a manual
   `workflow_dispatch` job) that runs step 3's checks, so runtime regressions are
   caught, not just compile errors.

## Quick reference
- Force a decode mode (support/debug): `STREMIO_HWDEC=nvdec|auto-safe|auto-copy|no`.
- Confirm the mode actually used: mpv logs `Using hardware decoding (<mode>)` at
  startup (the app sets `terminal=yes`). `nvdec` = zero-copy; `*-copy` = copy-back.
- See BENCHMARKS.md for numbers and LESSONS-LEARNED.md for the host-vs-flatpak story.
