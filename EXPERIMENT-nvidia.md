# Nvidia playback-CPU: experiment plan + status

The AMD/Intel fix is solved and shipping-ready (PR #108: `GSK_RENDERER=gl`,
`hwdec` zero-copy, main-loop-sleep fix). **Nvidia is the open problem** and this
doc is the single source of truth to drive it — scoped so a fresh narrow-context
agent can pick it up without re-deriving anything.

## The split (why Nvidia is different)

| | AMD / Intel (Mesa) | Nvidia proprietary |
| --- | --- | --- |
| WebKit UI render | GPU (Skia) → a cheap **GskTextureNode** | **software** → a **GskCairoNode** |
| Cost of UI-over-video composite | negligible (GPU blit) | **~1 pinned core** (GSK re-rasterises the software node every video frame) |
| Fix needed | PR #108 alone → CPU 2.5→0.3 core (KemalK, verified) | PR #108 **+** something for the UI recomposite |
| hwdec zero-copy (`auto-safe`) | VAAPI dmabuf: verified good | **nvdec/cuda: video artifacts** on 4K/HDR |

So Nvidia has **two independent problems**: (1) the UI-recomposite CPU cost, and
(2) hwdec zero-copy artifacts. They must not be conflated.

## What is fixed / in place (this branch, `feat/freeze-overlay`)

- **(2) hwdec artifacts — FIXED & VERIFIED on hardware (2026-07-19).** On Nvidia
  (`/dev/nvidia0` present) the init default and the web-UI remap both keep
  **copy-back** (`auto-copy`); Mesa keeps zero-copy (`auto-safe`).
  `src/app/video/imp.rs` + `src/app/video/mod.rs`. Verified live on the GTX 1060 @
  170% playing 4K HDR (The Dark Knight): **the video image is clean** — no
  stale/torn/garbled frames. Slightly higher decode CPU than zero-copy but
  correct. The user confirmed artifacts are "not in the video" — the remaining
  artifacts are **only in the UI overlay** (problem (1)).
- **(1) UI recomposite — `FreezeOverlay`, gated behind `STREMIO_FREEZE_OVERLAY=1`.**
  A wrapper that removes the WebKit overlay from the scene while the player chrome
  is hidden (driven by a `shell_ui` DOM observer on stremio-web's `overlayHidden`
  class → freeze controller in `app/imp.rs`). Default OFF so the shipping build is
  the artifact-free PR #108 baseline.

## Benchmarks (real GTX 1060, `GSK_RENDERER=gl`)

Controlled micro-bench (`examples/overlay_bench.rs`, isolates overlay-composite
cost; self-CPU from `/proc/self/stat`):

| overlay mode | CPU @2560×1440 | fps |
| --- | --- | --- |
| `none` (video only, the floor) | ~20.5% | 60 |
| `cairo` (today's software UI overlay) | **88.5%** | 20 |
| `frozen` (FreezeOverlay, hidden) | **21.6%** | 60 |
| `texture` (ideal GPU-texture UI) | ~20.1% | 60 |

Live 4K-HDR playback (user-driven): frozen (controls hidden) held **~20.8% CPU**
vs one pinned core unfrozen. **The CPU win is real and validated.**

## The blocking problem with FreezeOverlay (Nvidia/Wayland)

When FreezeOverlay hides the UI, **the WebKit overlay's last frame stays stuck on
screen** (stale top-bar / seekbar / popups that "never redraw out"), and playback
is choppy while the overlay is *shown*. Observed by the user:

- Overlay **hidden/cleared** → smooth, no artifacts, floor CPU.
- Overlay **shown** (unfrozen, or stale) → artifacts + choppy.
- **A full window remap clears it**: toggling fullscreen, or switching to another
  window and back. Moving the mouse re-shows the overlay and the artifacts return.

**Root cause (working theory):** WebKitGTK presents the UI through a **Wayland
subsurface** that GSK's `snapshot()` does not control. Skipping the snapshot stops
the CPU recomposite (the win) but does **not** tear down or re-commit that
subsurface, so its last buffer keeps being composited by KWin until a full surface
re-negotiation (what a resize / remap forces).

### Fixes already tried — all FAILED to clear the stale frame

1. `snapshot()` early-return only (zero nodes). Stale frame remains.
2. `set_visible(false)` on the child (`graphics_offload(webview)`). No change.
3. Wrap the WebView **directly**, `set_visible(false)` on it. No change.
4. `WEBKIT_DISABLE_DMABUF_RENDERER=1` (software WebKit, no dmabuf). Slightly
   smoother, **still stuck**.
5. `queue_resize()` on the child on the freeze transition — **FAILED**: did not
   clear the stale subsurface and caused intermittent **black screens**. Reverted.

Only a real remap (fullscreen toggle / window switch) has ever cleared it.

**Live baseline observation (2026-07-19, freeze OFF, copy-back hwdec, 4K HDR).**
With FreezeOverlay disabled — i.e. the plain shipping path — the user reports that
whenever the overlay chrome is *shown* (seekbar, top bar, Playback Speed / Statistics
popups), **playback becomes choppy and the overlay itself does not redraw cleanly**
(stale/laggy). The video image is clean. This is the raw, unmitigated software-
recomposite cost (problem (1)) with nothing suppressing it: the shown overlay is
expensive *and* its own updates lag under the CPU saturation. So the target isn't
only "hide the overlay cleanly" — even the *shown* overlay is degraded on Nvidia.
This reframes E4 ("don't hide; make it cheap") as equally important to E0–E3.

**Confirmed Nvidia-proprietary-specific** (not the GPU/Wayland stack): the UI is a
software `GskCairoNode` only on the proprietary driver (`webkit://gpu`; KemalK's
Intel test needed zero overlay work at 0.3-core). On Mesa the UI is a GPU texture,
so neither the CPU cost nor the artifact exists — FreezeOverlay isn't even run
there (`STREMIO_FREEZE_OVERLAY` off + `/dev/nvidia0` guard).

## Upstream state (researched 2026-07-19 — read this before experimenting)

The upstream situation is now well-established and it reframes the whole effort:
**this is a deliberate WONTFIX to work around, not a bug awaiting an upstream fix.**

**The load-bearing bug — WebKit #262607, `[GTK] Disable DMABuf renderer for
NVIDIA proprietary drivers` — is RESOLVED WONTFIX** (last activity 2024-09-18,
https://bugs.webkit.org/show_bug.cgi?id=262607). Igalia *intentionally* disables
the DMABuf/GPU renderer path when it detects the NVIDIA proprietary driver,
because it produced blank/garbled output for many users. The disable is keyed on
the **driver vendor string, not on a driver version or a fixable bug** — so even a
future fixed driver would not flip WebKit back to the GPU path without an upstream
WebKit change, and none is planned. This is *the* reason the Stremio UI is a
software `GskCairoNode` on Nvidia. It will not change on any timeline that helps
us.

| Upstream item | Status | Meaning for us |
| --- | --- | --- |
| WebKit **#262607** (disable DMABuf on Nvidia) | **RESOLVED WONTFIX**, 2024-09 | Root cause; deliberate; won't be reverted. |
| WebKit **#180739** (Nvidia + accelerated compositing) | NEW, abandoned since 2022 | Old umbrella; workaround is `WEBKIT_DISABLE_COMPOSITING_MODE=1` (kills HW accel entirely). |
| Stale-subsurface "last frame stuck until remap" | **No upstream bug exists** | Not tracked anywhere (WebKit/GTK/mutter). If we want it fixed we must file it with a minimal repro (`GDK_DEBUG=dmabuf,offload`). |
| WebKitGTK Skia GPU path (2.46→2.50) | Improving, but **never re-enabled on Nvidia proprietary** | Skia default since 2.46; 2.50 added an experimental hybrid GPU→CPU fallback; none touches the Nvidia block. No env var force-enables GPU on Nvidia. |

**One genuinely new lever — WebKitGTK 2.50 (Nov 2025) enabled "damage propagation
to the system compositor" by default** (present since 2.48, off until 2.50 —
https://webkitgtk.org/2025/11/26/webkitgtk-2.50.html). This is the only recent
change touching how WebKit's surface talks to the compositor. It is *not* a fix
for a hide-doesn't-recommit case, but it is worth testing against the
stale-overlay symptom on a 2.50+ WebKitGTK — see **E6**.

**Nvidia driver reality (decisive for this GTX 1060 / Pascal):**
- The **590 branch DROPS Maxwell/Pascal/Volta** (beta 590.44.01 Dec 2025, stable
  590.48.01 Jan 2026) — the GeForce 10 series is explicitly removed.
- **580 is the terminal proprietary branch** for this card: security-fix-only
  (quarterly, ~through 2028), no more feature work. We are on **580.159.03**;
  there is no newer feature branch to move to. So **no driver update will ever
  change WebKit's behaviour on this GPU** (and the block is vendor-string-based
  anyway).
- **nouveau/NVK** is Vulkan-1.4 conformant and default on Pascal since Mesa 25.1,
  **but Pascal is locked at boot clocks (no reclocking)** → poor throughput, and
  the NVK+Zink GL default is Turing+ only (Pascal still uses legacy nouveau GL).
  ⇒ nouveau is a **valid diagnostic boot** (does WebKit GPU-render there?
  very likely yes) but **not a shippable runtime** for smooth video on this card.

**Consequence for the plan:** plan for a **client-side workaround, not an upstream
fix**. The realistic levers all live in our own code (FreezeOverlay-style
suppression + a reliable subsurface recommit/remap, or reducing the shown
overlay's cost). E0 (nouveau) now answers a narrower question — *confirm the root
cause* — rather than *find the daily fix*; a GO there means "the proprietary
driver's software-WebKit is the whole problem," not "boot nouveau to ship."

## Is this a dead end? (the honest question)

Maybe. The Nvidia UI-overlay is on a subsurface we can suppress (CPU win) but not
cleanly hide (artifacts). If no reliable "force WebKit to re-commit / drop its
subsurface" mechanism exists short of a remap, FreezeOverlay is a dead end and the
Nvidia UI cost is an **upstream WebKit limitation** (won't GPU-render on Nvidia).
But several avenues remain untried — the point of the experiment plan.

## Experiment plan (drive Nvidia development — do in order, cheapest/riskiest first)

Each experiment: state the hypothesis, the one thing it tests, and a clear
GO/NO-GO. Reuse `STREMIO_FREEZE_OVERLAY=1` + `RUST_LOG=freeze=debug`
(`apply frozen=…` logs). **Pre-flight EVERY launch:** `ss -ltnp | grep :11470`
must show the shell's own node server (kill orphans first — see DEVLOG "Lessons
learned").

**E0 — Nouveau/NVK driver diagnostic (do FIRST — decides everything).** Boot the
same Nvidia hardware on the open **nouveau/NVK** (Mesa) driver instead of the
proprietary one (blacklist `nvidia`, reboot — a user action). Play a file with
`STREMIO_FREEZE_OVERLAY` OFF and check `webkit://gpu`. **Hypothesis:** on Mesa
WebKit GPU-renders → the UI is a cheap texture → **both the CPU cost and the
overlay artifact vanish with no FreezeOverlay needed**, exactly like AMD/Intel.
GO ⇒ proves the whole problem is the proprietary driver's software-WebKit
fallback (WONTFIX #262607) — the fix is necessarily **client-side** for users
stuck on proprietary, and FreezeOverlay is the stopgap. NO-GO (still
software/artifacting on nouveau) ⇒ the problem is deeper (GPU/Wayland), reopen the
subsurface theory. Cheap, non-code, highest information-per-effort. **Caveat from
the upstream research:** nouveau on Pascal has no reclocking (boot clocks only) so
video will be rough — this boot is *only* to read `webkit://gpu` and eyeball
whether the UI is a GPU texture, **not** a runtime we can ship. Coordinate the
reboot with the user (blacklist `nvidia`, reboot is a user action).

**E1 — Force a real re-commit on freeze — DONE, FAILED.** `queue_resize` did not
clear the stale subsurface and caused black screens (reverted). A heavier real
size change (allocate 1px-different for one frame then back) could still be tried,
but E0/E2/E3 are more promising.

**E2 — Reset the WebView surface via WebKit.** Investigate whether WebKitGTK
exposes a way to drop/re-create its rendering surface on hide (e.g. toggling
`WebView` mapped state via `unrealize`/`realize`, or a WebKit setting). GO if
hide→show yields a fresh surface with no stale frame. Cost: possible re-init cost.

**E3 — Determine the subsurface truth.** Run with `GDK_DEBUG=offload` during
playback and confirm whether the WebView (and/or the video GLArea) is actually on
a subsurface at 170% scale, and which node type it produces frozen vs unfrozen.
This decides E1/E2 vs E4. Cheap, do early.

**E4 — Don't hide; make it cheap instead.** If hiding is fundamentally broken,
revisit reducing the *cost* of the shown overlay: force WebKit acceleration, or a
`GtkGraphicsOffload` on the video (blocked at fractional scale — but re-verify per
GTK version), or accept the cost and only reduce *frequency* differently.

**E5 — Upstream reality check — DONE (2026-07-19).** Answered by the research
above: **#262607 is RESOLVED WONTFIX** (deliberate, vendor-string-based) and
Skia-on-Nvidia has **not** moved through 2.50. No env var force-enables the GPU
path. There is no upstream fix coming and no driver update can change it (Pascal
is EOL on 580; 590 drops the card). ⇒ **The fix must be client-side.** Do not wait
on upstream. Re-check only if a future WebKitGTK release note explicitly mentions
re-enabling GPU rendering on the Nvidia proprietary driver.

**E6 — WebKitGTK 2.50 compositor damage propagation — LARGELY FALSIFIED by the
environment.** 2.50 (Nov 2025) enables "damage propagation to the system
compositor" by default. The hypothesis was that accurate damage might let the
compositor drop the stale subsurface region instead of holding the last frame.
**But the installed WebKitGTK is already 2.52.3** (`webkitgtk-6.0`,
Ubuntu 26.04) — i.e. damage propagation was **already ON** when the user observed
the stale-overlay artifact. So this lever, as a passive default, does **not** fix
the artifact. Residual value is small: check only whether WebKitGTK 2.52 exposes
an *explicit* API to force a damage/recommit on hide (unlikely). Effectively:
**do not count on an upstream/WebKit-version fix for the stale frame — it is
present on the newest WebKitGTK.** Deprioritise; E0/E3 and a client-side
recommit/remap remain the real work.

## Guardrails for the next agent

- **Keep the AMD/Intel fix intact** (PR #108 behaviour). All Nvidia work is behind
  `STREMIO_FREEZE_OVERLAY` and `/dev/nvidia0` guards — do not change the Mesa path.
- **Do not conflate** the two Nvidia problems (UI recomposite vs hwdec artifacts).
- **Verify server on :11470 before every playback test.** This has bitten twice.
- The user tests playback by hand (desktop input automation is unreliable here);
  read `apply frozen=…` from the log and ask the user to eyeball visuals.
