> **★ SUPERSEDED / RESOLVED (2026-07-19).** This whole doc chased the wrong
> problem: the NVIDIA cost was **copy-back video decode**, not the UI overlay, and
> nearly all the "overlay/renderer" symptoms below were **host-only** (they don't
> reproduce in the flatpak). The overlay is ~1% in the shipping flatpak.
> **Fix: zero-copy `nvdec`** → 4K HDR ~100%→~23%, clean. See LESSONS-LEARNED.md,
> BENCHMARKS.md, and `default_hwdec()` in `src/app/video/mod.rs`. `FreezeOverlay`
> is abandoned. Kept below for history.

# NVIDIA UI-overlay: decision + plan

Reviewed 2026-07-19 against `EXPERIMENT-nvidia.md`, ADR-0003/0004/0005, and
`src/app/freeze_overlay/`. Scope: problem (1) only (UI recomposite / stale
overlay). Problem (2) hwdec is fixed — untouched. Mesa/PR#108 path — untouched.

## 1. Verdict

**Conditionally fixable client-side — not yet a dead end — and ONE diagnostic
session decides it.** The CPU win (FreezeOverlay, 88.5%→21.6%) is banked; the
only blocker is the stale-frame artifact, and its root cause is **not actually
established**. The doc's "WebKit Wayland subsurface" theory is a working theory
that contradicts two verified facts already in this repo:

- ADR-0003 (verified in GTK 4.22.4 source *and* the shipped binary): GDK
  **declines every subsurface at fractional scale** (`scaled_rect_is_integral`).
  At 170% there should be *no* GDK subsurface to go stale. WebKitGTK 6.0 renders
  into the GTK scene graph (that is *why* it costs CPU as a `GskCairoNode`) — it
  owns no subsurface of its own in this port.
- "Switching to another window and back clears it" — an alt-tab remaps nothing
  and forces no client recommit. A live stale subsurface would survive it. This
  observation points at **compositor-side or client-backbuffer state**, not a
  held subsurface buffer.

So the stale pixels live in one of three places, and each has a *different* fix:

| Mechanism | Where the stale pixels are | Client-side fix exists? |
| --- | --- | --- |
| **A. Real subsurface** (theory as written) | a wl_subsurface buffer KWin keeps compositing | Probably (unrealize/reparent tears it down) |
| **B. Our own backbuffer / swap-damage** (GSK-on-NVIDIA-EGL buffer-age or damage-reporting bug) | the shell's committed buffer, or KWin's texture updated only within reported damage | Yes, cheap ("damage bomb") |
| **C. Compositor-side** (KWin+NVIDIA texture/scanout cache) | KWin's GL texture for our surface | Essentially no (only remap-class tricks) |

If diagnosis lands on A or B: **fixable, ship FreezeOverlay default-on for
NVIDIA.** If C: **dead end** — ship the PR#108 baseline on NVIDIA (freeze OFF,
status quo, upstream parity), keep `STREMIO_FREEZE_OVERLAY=1` as a documented
power-user opt-in, and file the compositor bug with the protocol logs from D1.
Do not ship FreezeOverlay with stale-frame caveats as default: a stuck seekbar
over the film is a worse disturbance than transient choppiness while chrome is
shown.

The single deciding experiment is **D1** below (~15 min of user-driven runs, no
code changes).

## 2. Critique of E0–E6

- **E0 (nouveau boot) — KILL.** It was framed as "decides everything"; it no
  longer decides anything. E5's research already proved the root cause
  (vendor-string WONTFIX #262607) and proved nouveau is unshippable on Pascal
  (boot clocks). A GO tells us what we already know; a NO-GO is uninterpretable
  (nouveau's stack differs in too many variables). Not worth a reboot. Park it
  only as a tie-breaker if D1's results are self-contradictory.
- **E1 (queue_resize) — dead, correctly buried.** Its heavier variant (1px
  allocate jitter) is also refuted in advance: allocation churn acts on the GSK
  scene, and the frozen child contributes zero nodes — there is nothing in the
  scene to re-commit. Do not resurrect.
- **E2 (WebKit-side surface reset) — KEEP, but sharpen.** "A WebKit setting"
  does not exist (webkit6 exposes no surface/present control; hw-accel policy
  Always was already tested in ADR-0003 and the overlay stayed a CairoNode).
  The only real lever in E2 is **`unrealize()`/`realize()` or full
  unparent/reparent** — strictly stronger than the failed `set_visible(false)`
  because it destroys the widget's GDK/EGL resources rather than just unmapping.
  Run it only if D1 says mechanism A.
- **E3 (subsurface truth) — KEEP, PROMOTE to first, and strengthen.**
  `GDK_DEBUG=offload` only prints GDK's offload decisions; it cannot see a
  hypothetical WebKit-owned surface and cannot distinguish B from C. Add
  `WAYLAND_DEBUG=1` (protocol ground truth: does *any* wl_subsurface exist at
  170%? does anything commit after freeze?) and a `GSK_DEBUG=full-redraw` A/B
  (kills or confirms B in one run). This upgraded E3 is D1 below.
- **E4 (make the shown overlay cheap) — KEEP as an independent second track.**
  The 2026-07-19 baseline observation makes it mandatory regardless of the
  stale-frame outcome: even perfect hiding leaves the *shown* chrome choppy.
  But prune it: "force WebKit acceleration" is falsified (ADR-0003), and
  re-verifying offload at fractional scale is falsified (the gate is
  fundamental, no MR relaxes it). The one live idea is **`webkit_web_view_get_snapshot()`**
  (IMPLEMENTATION_PLAN's Contingency B): it renders in the WebProcess,
  independent of the UI-process presentation path that defeats
  `WidgetPaintable::current_image()`, so it plausibly returns real pixels on
  NVIDIA. One cheap probe decides it (D5).
- **E5, E6 — done, correct, closed.** Agree with both conclusions. The 2.52.3
  fact (damage propagation already on when the artifact was observed) is
  well-used; note it also *mildly supports* mechanism B/C — accurate damage
  reporting plus a broken consumer is exactly how partial-upload staleness
  happens.

Ordering: **D1 → (exactly one of D2/D3/D4) → D5.** Everything else is dead
weight.

## 3. The core question: forcing a release/recommit short of a full remap

Candidate by candidate, at 170%:

1. **Unrealize/realize the WebView** (`WidgetExt::unrealize()` on freeze,
   `realize()`+`queue_draw` on unfreeze). Mechanism: destroys the widget's GDK
   surface-attached resources; if WebKit's presentation holds any per-realize
   surface state (mechanism A), this tears it down — the only widget-level op
   stronger than unmap short of unparenting. Nothing about it is
   scale-dependent. Risks: WebKit re-init cost / white flash on unfreeze; GTK
   documents unrealize as widget-implementation API, so guard against asserts
   (unmap first). Falsify in one test: freeze → stale gone? unfreeze → live UI
   within one frame, no flash? Irrelevant if D1 says B or C (nothing realized
   holds the pixels then).
2. **Unparent to an offscreen holder / reparent back.** Same mechanism as (1)
   but guaranteed teardown. Heavier risks (focus, WebKit GL context churn).
   Only as fallback if (1) clears the stale frame but flashes.
3. **WebKit-side surface reset via API.** Does not exist in webkit6. Dead.
4. **Forcing damage/recommit ("damage bomb").** Mechanism: if the stale pixels
   are ours (B) — GSK skipping the repaint via buffer-age, or reporting
   too-small swap damage so KWin only partially re-uploads — then forcing 2–3
   consecutive **full-window** redraws right after the freeze transition
   flushes every buffer in flight and every compositor texel. Implement as a
   short-lived tick callback that invalidates the full window (or a transient
   full-window node change). Scale-independent, imperceptible (the frames drawn
   are identical minus the overlay), ~20 lines. Falsified in advance by D1b: if
   `GSK_DEBUG=full-redraw` doesn't clear the artifact, B is dead and so is this.
5. **Toggling WebView mapped state** — already failed (`set_visible(false)`,
   tries 2–3). Dead.
6. **Deliberate minimal imperceptible remap.** Does not exist on Wayland/KWin:
   resize is impossible fullscreen, decoration/fullscreen toggles flash,
   hide/show flashes. If only remap-class actions work (mechanism C), there is
   no imperceptible variant — that *is* the dead-end branch.
7. **`GDK_DEBUG=offload` / damage angle** — diagnostic only, folded into D1.
8. **Attack the SHOWN overlay instead (E4/snapshot).** Orthogonal: even if
   stale-hiding is fixed, shown chrome stays choppy; even if hiding is dead,
   a throttled snapshot-texture overlay could replace the live CairoNode
   entirely. Kept as its own probe (D5). Note: it does not by itself fix the
   stale frame — whatever presents the stale pixels must still be silenced.

Also worth one free A/B while instrumented: **`GSK_RENDERER=vulkan` (NVIDIA
only)**. If the stale frame is an ngl/EGL buffer-age artifact (B), the Vulkan
renderer's different swapchain/damage path may simply not have it. Zero code;
does not touch the Mesa `gl` default.

## 4. Concrete next actions

Pre-flight every run: `ss -ltnp | grep :11470` shows our own server;
`STREMIO_FREEZE_OVERLAY=1`, `RUST_LOG=freeze=debug`; user drives playback and
eyeballs; all runs at 170% on the proprietary driver.

- **D1 — Mechanism diagnosis (DO FIRST; decides A/B/C; no code).** Four short
  launches, same clip, freeze/unfreeze via mouse-idle each time:
  - *D1a* `WAYLAND_DEBUG=1 …app… 2>wl.log`; grep for `wl_subsurface` /
    `zwp_linux_dmabuf` commits around the freeze timestamp. Tests: does any
    subsurface exist at 170%, and does anything keep committing after freeze?
  - *D1b* `GSK_DEBUG=full-redraw`. Tests: is the stale frame gone when every
    frame is a full repaint?
  - *D1c* `GDK_DEBUG=offload,dmabuf` for the record (expected: all offload
    declined).
  - *D1d* `GSK_RENDERER=vulkan`. Tests: does the artifact (and/or the
    shown-overlay choppiness) differ under the Vulkan renderer?
  - **Decision:** subsurface present in D1a → **A → D2**. No subsurface and
    D1b clears the artifact → **B → D3**. No subsurface and D1b does NOT clear
    it → **C → D4**. (D1d GO on both symptoms would be a shippable shortcut:
    NVIDIA-gated `GSK_RENDERER=vulkan` — verify no regressions, then ship.)
- **D2 — (only if A) unrealize/realize on freeze.** Hypothesis: destroying
  realize-scoped resources tears down whatever presents the stale buffer.
  One test: freeze → artifact gone; unfreeze → live UI, no flash, CPU still
  ~21%. GO → productionize inside `FreezeOverlay::set_frozen` (NVIDIA-gated).
  NO-GO → one attempt at unparent/reparent; if that also fails, treat as C.
- **D3 — (only if B) damage bomb.** Hypothesis: 2–3 forced full-window redraws
  after the freeze transition flush the stale pixels everywhere. Prototype in
  `set_frozen` (tick callback, full invalidate, self-removes). One test:
  freeze → artifact gone at 170%, CPU returns to floor after the burst.
  GO → ship FreezeOverlay default-on for NVIDIA. NO-GO → re-read D1b (it
  predicted GO; a conflict means the mechanism table is wrong — stop, re-diagnose).
- **D4 — (only if C) declare the dead end.** No further prototypes. Ship
  baseline (freeze OFF) on NVIDIA; keep `STREMIO_FREEZE_OVERLAY=1` documented
  as opt-in ("controls may stick; toggle fullscreen to clear"); file the
  upstream bug (KWin or GTK) with D1a's protocol log + minimal repro — the doc
  already notes no such bug exists anywhere.
- **D5 — (independent, any time after D1) snapshot probe for the shown
  overlay.** Hypothesis: `webkit_web_view_get_snapshot()` returns real pixels
  on NVIDIA (WebProcess render, bypasses the presentation path that broke
  `current_image()`). Probe: one debug keybinding/env that calls it during
  playback and logs texture size + saves a PNG. GO → design "SnapshotOverlay"
  (throttled ~10 Hz texture while chrome shown, live CairoNode never in scene)
  as the E4 fix for shown-overlay choppiness. NO-GO (empty/black) → E4 is
  dead; accept transient choppiness while chrome is shown.

Guardrails unchanged: all behavior NVIDIA-gated (`/dev/nvidia0`), Mesa path
byte-identical, the two NVIDIA problems stay separate.

**Bottom line:** run D1 once — it costs 15 minutes, no code, and converts
"maybe a dead end" into exactly one of: a 20-line fix (B), a one-widget fix
(A), or a justified, evidence-backed decision to ship the baseline and file
upstream (C).

---

## 5. Final empirical results (2026-07-19) — VERDICT: ship baseline, hand off

D1 and the follow-ups were run live on the GTX 1060 @ 170%, proprietary 580.159.03.
Outcome: **mechanism B (GSK NGL buffer-age), but no clean surgical trigger exists —
FreezeOverlay is NOT shippable. Ship the PR #108 baseline (freeze OFF).**

**What we measured:**
- **D1a (`WAYLAND_DEBUG=1`)** — **0 `wl_subsurface` in a 40,769-line trace.**
  Mechanism **A (subsurface) is DEAD** — WebKitGTK 6.0 owns no subsurface here.
  Damage is **full-window every frame** (3,495× `damage_buffer(0,0,3840,2161)`),
  so KWin is correctly told the whole surface changed → **mechanism C (compositor
  under-read) is very unlikely**. A **4-buffer pool** cycles (`#75/#77/#84/#85`).
- **D1b (`GSK_DEBUG=full-redraw`)** — **clears the stale frame** (hidden overlay is
  clean + smooth). Confirms **mechanism B**: GSK's NGL renderer partial-repaints
  into the recycled pool and fails to repaint the vacated overlay region across
  all buffers. BUT continuous full-redraw **causes black screens when the overlay
  is *shown* in fullscreen** (it force-re-rasterizes the 4K software overlay every
  frame) — so it can't ship as an always-on flag.
- **D3 (damage-bomb, coded)** — a post-freeze burst of full-bounds, alpha-varying
  nodes to force GSK to flush the pool. **NO-GO:** a near-transparent node on top
  doesn't make GSK re-render the *video* underneath (GSK re-blends the node over
  the retained buffer). Reverted.
- **D1d (`GSK_RENDERER=vulkan`)** — **worse**: stale frame persists AND black
  screens even without the overlay. NO-GO.
- **Integer scale (`GDK_SCALE=1`)** — stale frame **persists**. So the bug is
  **NOT fractional-scale-specific** (rules out an integer-render workaround).
- **`GDK_DEBUG=gl-no-fractional`** — **does not exist** on GTK 4.22.4 (absent from
  `GDK_DEBUG` and `GDK_DISABLE`). `GSK_RENDERER=cairo` would software-composite
  the 4K video every frame → non-starter for a video app.

**Upstream anchor:** this is **[GNOME/gtk #4676](https://gitlab.gnome.org/GNOME/gtk/-/issues/4676)**
(OPEN since 2022) — "GTK gets a new empty buffer from EGL and doesn't fully
repaint… buffer_age check", acknowledged workaround `GSK_DEBUG=full-redraw`.
Exactly our finding. Related: gtk #6091, #6703, Ubuntu #2061079, and an NVIDIA
forum report that the repaint bug **survives into the 590 branch**. Root-caused to
NVIDIA's closed EGL/GL buffer-pool × GTK buffer-age.

**Drivers (researched):** no help. `nvidia-open` is **impossible on Pascal**
(needs Turing+ GSP) and uses **identical proprietary userspace** anyway (would
behave the same). 580.x is the terminal branch; **590 drops Pascal**. **nouveau/NVK**
is the only stack that could change GSK's behaviour (different GL userspace) but
Pascal has **no reclocking** → rough video, diagnostic-only, unshippable.

**The two NVIDIA problems, final status:**
1. **Video-decode artifacts → FIXED** (copy-back hwdec), verified clean on 4K HDR.
   → ships in PR #108.
2. **UI-overlay recomposite → UNFIXABLE cleanly here.** The CPU win (FreezeOverlay,
   88.5%→21.6%) is real, but hiding the overlay leaves a stale frame that only a
   full remap / continuous full-redraw clears — and full-redraw black-screens the
   shown overlay. No shippable fix exists on GTX 1060 + proprietary + KWin.
   → FreezeOverlay is **dropped** (too buggy even as an opt-in) and handed to
   upstream/other devs as a **patch to try**, with this diagnosis, on PR #108.

**Decision:** ship PR #108 (freeze OFF) as the NVIDIA path — fixed video, no
artifacts, no black screens, upstream parity. Document everything here (dev
branch); push the copy-back fix to the PR; attach the FreezeOverlay diff + this
diagnosis + gtk #4676 to the PR for others to attempt.
