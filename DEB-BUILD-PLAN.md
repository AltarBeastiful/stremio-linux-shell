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
**Audited 2026-07-19** in fresh `ubuntu:<codename>` containers (both halves —
libmpv `CUDA hwdec` string AND libavcodec `nvdec`/`cuvid`). **All three targets
pass both halves**, so every target is flagged `"expect_nvdec": true` in
`releases.json`:

| release | libmpv2 | CUDA interop | ffnvcodec (libavcodec) |
| --- | --- | --- | --- |
| noble 24.04 | 0.37.0-1ubuntu4 | ✅ | ✅ (av1/h264/hevc `_nvdec` + `_cuvid`) |
| resolute 26.04 | 0.41.0-2ubuntu4 | ✅ | ✅ (av1/h264/hevc `_nvdec` + `_cuvid`) |
| stonking 26.10 | 0.41.0-2ubuntu4 | ✅ | ✅ (av1/h264/hevc `_nvdec` + `_cuvid`) |

So **NVIDIA users get the zero-copy win on every current target** (24.04 was the
open question — now confirmed). Any future target release needs the same audit
before it is flagged `expect_nvdec: true`.

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

**Can WebKitGTK ≥ 2.52.4 be a hard install requirement (min version)?** Tempting,
but a blanket `Depends: libwebkitgtk-6.0-4 (>= 2.52.4)` is wrong *today*:
- The 26.04 archive still ships **2.52.3**, so the floor is satisfiable on no
  current release and the package becomes **uninstallable for everyone** (AMD/Intel
  included) — exactly the failure `verify-deb.sh` TEST 1 exists to catch, and what
  the `Cargo.toml` "do not pin" comment forbids.
- The lag is **NVIDIA-only**, so a global `Depends` punishes every GPU for an
  NVIDIA-only issue. The .deb is one package for all GPUs; it cannot depend
  conditionally on the GPU.
- `$auto` will **not** add this floor by itself: 2.52.4 is a rendering bugfix with
  unchanged ABI/symbols, so `dpkg-shlibdeps` sees no reason to raise the floor. It
  must be declared explicitly if/when we want it.

**So the requirement is staged, not immediate:**
- **Now:** document ≥ 2.52.4 for NVIDIA in `packaging/README.md`, and have
  `data/stremio.sh` warn at launch when it detects NVIDIA (`/dev/nvidia0`) **and**
  system WebKitGTK < 2.52.4 (point at the flatpak meanwhile). A soft runtime notice
  keeps the .deb installable everywhere while still telling affected users why the
  UI lags.
- **Once the WebKit SRU has landed in every target's `-updates` pocket** (2.52.4+
  in both `noble-updates` and `resolute-updates`), add the explicit versioned floor
  `libwebkitgtk-6.0-4 (>= 2.52.4)` as an extra `depends` entry — by then it is
  satisfiable, so `verify-deb.sh` TEST 1 still passes and NVIDIA correctness is
  guaranteed at install time. Track the SRU before flipping this on.

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

Each step is tagged with the hardware it needs, since the .deb work is done on an
AMD machine that cannot test NVIDIA locally: `[any machine]` (container/docs/script,
no GPU), `[NVIDIA box]`, `[wait: SRU]` (calendar, no hardware).

1. **Audit CUDA decode per target release (B1).** `[any machine — container]`
   The requirement has **two halves** — libmpv cuda-interop AND ffmpeg ffnvcodec —
   and libmpv links libavcodec dynamically, so checking libmpv alone proves nothing
   about the ffmpeg half. In a fresh container of each `releases.json` target check
   both (use `grep -a`, not `strings` — a fresh container has no binutils):
   - libmpv cuda-interop:
     `grep -a 'CUDA hwdec' $(dpkg -L libmpv2 | grep 'libmpv\.so\.2\.[0-9]')`
   - ffmpeg ffnvcodec:
     `grep -a -m1 -E 'nvdec|cuvid' /usr/lib/*/libavcodec.so.*`
   Both must hit. **DONE (2026-07-19):** all three targets — noble 24.04, resolute
   26.04, stonking 26.10 — pass both halves (see B1 table). Each is flagged
   `"expect_nvdec": true` in `releases.json`. 26.10 (stonking, beta) was added this
   session at the user's request; it reuses `api-4_22` and already ships
   WebKitGTK 2.52.4.
2. **Add a build-time smoke check (B1/B3).** `[any machine]`
   Extend `packaging/verify-deb.sh` with the two `grep -a` checks from step 1
   against the installed libs. **FAIL only when `releases.json` says
   `expect_nvdec: true`** for that target (regression guard); otherwise WARN. A
   blanket hard-fail would block the whole .deb for AMD/Intel users over an
   NVIDIA-only shortfall — the same "don't punish every GPU" logic that rules out
   the hard WebKit `Depends` in B2.
3. **Soft WebKit requirement + docs (B2, soft half).** `[any machine]`
   - Document in `packaging/README.md`: the libmpv CUDA-interop requirement, the
     **WebKitGTK ≥ 2.52.4 requirement for NVIDIA**, the `STREMIO_HWDEC` override for
     support/debug, the deb-vs-flatpak NVIDIA caveat, and the AV1 note below.
   - Launch-time warning in `data/stremio.sh` — the .deb only runs on dpkg systems,
     so query dpkg (no pkg-config on user machines):
     ```sh
     wk=$(dpkg-query -W -f='${source:Upstream-Version}' libwebkitgtk-6.0-4 2>/dev/null)
     if [ -e /dev/nvidia0 ] && [ -n "$wk" ] && dpkg --compare-versions "$wk" lt 2.52.4; then
       echo "stremio: WebKitGTK $wk < 2.52.4 on NVIDIA — UI may lag/artifact; the flatpak is unaffected." >&2
     fi
     ```
     Fires spuriously on hybrid laptops rendering on the iGPU — accepted, matches
     the existing `GSK_RENDERER`/`/dev/nvidia0` detection. Dry-run testable on AMD
     by overriding `$wk` and the `/dev/nvidia0` check.
   - **AV1 on pre-Ampere NVIDIA:** AV1 hardware decode starts at **Ampere (RTX 30)**;
     Turing (RTX 20 / GTX 16), Pascal (GTX 10) and older have **no AV1 decoder**, so
     AV1 falls back to software decode (heavy at 4K) regardless of `nvdec`/deb/flatpak
     — a GPU limit, not a bug. `nvdec` zero-copy still applies to H.264/HEVC.
4. **Per-distro NVIDIA runtime validation (B1/B2/B3).** `[NVIDIA box]`
   **Runbook: `packaging/nvidia-runtime-check.sh <deb>`** automates install +
   the system nvdec-capability check + the WebKit-version caveat, then prints the
   manual playback checklist below. Install each release's deb on NVIDIA, play a
   4K HDR title fullscreen:
   - (a) mpv logs `Using hardware decoding (nvdec)` (not `*-copy`); (b) `stremio`
     process CPU ~20–25%, A/B vs `STREMIO_HWDEC=auto-copy`. **(a)+(b) must pass now**
     — they are independent of WebKit.
   - (c) laggy UI / rendering artifacts — **EXPECTED TO FAIL** with the archive's
     WebKitGTK 2.52.3. This is not a regression. Confirm B2 by A/B: archive 2.52.3
     (lag expected → confirms the diagnosis), then add Jeremy Bícha's `webkit2gtk`
     PPA (2.52.5) and re-run (c) (clean expected → confirms the fix). Only after the
     PPA run is B2 root cause CONFIRMED — no WebKit release note names this artifact,
     so this empirical A/B is the decider. Remove the PPA after.
5. **Hard WebKit floor (B2).** `[wait: SRU — no hardware]`
   Trigger: `rmadison libwebkitgtk-6.0-4` (or launchpad.net/ubuntu/+source/webkitgtk)
   shows ≥ 2.52.4 in **both** `noble-updates` and `resolute-updates`.
   **Checked 2026-07-19 (still BLOCKED):** noble-updates has `2.52.3-0ubuntu0.24.04.1`,
   resolute-updates has `2.52.3-0ubuntu0.26.04.2` — both 2.52.3, below the floor.
   (The SRU pathway is active: 2.52.3 is already in -updates over the base pockets'
   2.44.0/2.52.0. stonking 26.10 already ships 2.52.4, so once noble/resolute catch
   up all three targets satisfy the floor.) Do **not** flip the floor until then.
   When the trigger fires, in one commit:
   - `depends = "$auto, nodejs, libwebkitgtk-6.0-4 (>= 2.52.4)"`. This **duplicates**
     the `$auto` entry (once with $auto's low floor, once explicit); dpkg/apt apply
     both constraints — that's fine, do **not** "deduplicate" the explicit floor.
   - Amend the `Cargo.toml` "do not pin" comment in the same commit: a floor is
     allowed once satisfiable from every target's `-updates` pocket (else a later
     cleanup reverts this, trusting the old comment).
   - Re-run `verify-deb.sh`; TEST 1 needs `-updates` enabled in the fresh container
     (official `ubuntu:*` images have it). Accepted trade-off: users with `-updates`
     disabled can't install.
6. **(Optional) extend CI.** A NVIDIA-tagged self-hosted runner (or a manual
   `workflow_dispatch` job) that runs step 4's checks, so runtime regressions are
   caught, not just compile errors. The automatable half is ready to wrap
   (`packaging/nvidia-runtime-check.sh`); the playback A/B (CPU + decode log)
   still needs a human at a display, so full automation is limited to the
   install + `check-nvdec.sh` + WebKit-version report. Deferred until a
   `[self-hosted, nvidia]` runner exists — the GH-hosted pool has no NVIDIA GPU.

## Implementation progress

Session 2026-07-19/20 (this branch, `develop`):
- **Step 1 DONE** — audited all three targets (noble/resolute/stonking); each
  `expect_nvdec: true` in `releases.json`. 26.10 (stonking) added at user request.
- **Step 2 DONE** — `packaging/check-nvdec.sh` (+ unit test) and `verify-deb.sh`
  TEST 8, wired through `test-deb.sh` and `release.yml` via `EXPECT_NVDEC`.
  Validated locally: noble + resolute + stonking build, install, and pass TEST 8.
- **Step 3 DONE** — launcher WebKit warning (`data/stremio.sh`, + unit test) and
  the NVIDIA/decode section in `packaging/README.md`.
- **Step 4 PENDING** — needs the NVIDIA box; runbook ready (see above).
- **Step 5 BLOCKED** — SRU not landed (checked 2026-07-19, see above).
- **Step 6 DEFERRED** — see above.

## Quick reference
- Force a decode mode (support/debug): `STREMIO_HWDEC=nvdec|auto-safe|auto-copy|no`.
- Confirm the mode actually used: mpv logs `Using hardware decoding (<mode>)` at
  startup (the app sets `terminal=yes`). `nvdec` = zero-copy; `*-copy` = copy-back.
- See BENCHMARKS.md for numbers and LESSONS-LEARNED.md for the host-vs-flatpak story.
