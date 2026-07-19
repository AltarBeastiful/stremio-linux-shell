# Implementation plan — eliminate overlay-compositing CPU on all platforms, **fractional scale included**

Status: **LOCKED** 2026-07-18 (design-reviewed; supersedes both the CachedOverlay
direction — ADR-0003, FALSIFIED — and the earlier two-candidate draft of this
file). See `BENCHMARKS.md` for measured numbers and `ADR.md` ADR-0003
"Update 2026-07-18" for the evidence trail.

**The plan: Candidate A — `FreezeOverlay` (empty-`snapshot()` scene exclusion of
the idle WebView) driven by a preload-injected DOM visibility observer
(`shell_ui`), with eager local unfreeze on input.** Candidate B survives only as
a contingency appendix with a single, precise trigger.

## Objective & hard constraints

Eliminate the pinned-CPU-core during playback on **every** GPU platform
(Nvidia proprietary, AMD/Mesa, Intel/Mesa) with **no exceptions and no user
disturbance**:

1. **Must work at fractional display scale** (e.g. 170%). Product requirement —
   fractional scale is the common case and cannot be sacrificed.
2. **No UX regression.** UI stays visible and interactive exactly as today;
   subtitles, seekbar, buffering feedback all keep working. Keyboard shortcuts
   (Space/arrows) must keep working **while the overlay is excluded**.
3. **All drivers.** No reliance on a WebKit-on-Nvidia or GTK-offload upstream fix.

## What is definitively ruled out (verified 2026-07-18 — do not re-try)

| Approach | Why it's dead | Evidence |
| --- | --- | --- |
| **Subsurface / `GtkGraphicsOffload` of the video** (incl. `GdkDmabufTextureBuilder`) | GTK declines subsurface attach at non-integral device coordinates; **fundamental to `fractional-scale-v1`** (undefined subsurface rounding), not a stray guard. Only works at integer scale. | GTK 4.22.4 `gdksubsurface-wayland.c` `scaled_rect_is_integral`, confirmed in source + shipped binary; live trace "Non-integral device coordinates" at 170%. NVIDIA 580 *does* export dmabuf — scale is the blocker, not export |
| **`CachedOverlay` / any `WidgetPaintable::current_image()` snapshot-cache of the WebView** | Renders **black** — `current_image()` **rasterizes the WebView's node tree empty** (the pixels are presented by WebKit through a path the GSK rasterizer cannot capture; note: not necessarily a literal subsurface — at 170% offload no-ops, yet capture is still empty). Tested in **both** default (dmabuf) **and** `WEBKIT_DISABLE_DMABUF_RENDERER=1` modes — black in both. | Live app A/B 2026-07-18; hover tooltip fires (widget live) but pixels never paint |
| **`set_visible(false)` / `set_child_visible(false)` on the WebView** | Unmaps the WebView → keyboard focus lost (Space/arrows die while controls hidden) and WebKit flips Page Visibility (`document.hidden`, rAF stops, timers throttle) → web player logic stalls, stale-content flicker on remap. Violates constraint 2. | Design review 2026-07-18 (GTK map/focus semantics + WebKitGTK activity state) |
| **Bump WebKitGTK / GTK / flatpak runtime** | Already newest (runtime 50 = GTK 4.22.4 + WebKit 2.52.x). WebKit GPU-on-Nvidia is upstream WONTFIX (#262607, #180739). | Research 2026-07-18 |
| **`KWIN_USE_OVERLAYS`** | Red herring: the CPU composite happens client-side in GSK before any buffer reaches KWin; overlay planes need the video on a dmabuf subsurface (blocked at 170%). Off-by-default due to Nvidia freezes. | Research 2026-07-18 |
| **`hardware-acceleration-policy=Always`, `WEBKIT_DISABLE_DMABUF_RENDERER`, force integer scale / `cairo` renderer** | Ineffective and/or user-hostile. | DEVLOG §11 |
| **Candidate B's original "offscreen WebView + forwarded events"** | GTK4 removed event synthesis (`gdk_event_new`/`gtk_main_do_event`); input cannot be forged to an off-scene widget. Any B variant must keep the WebView in-scene and mapped. | Design review 2026-07-18 |

## The core insight, and why the mechanism is already proven

The waste is **GSK re-compositing the UI node every frame because the video node
under it changes**. Every "separate the layers" trick needs a subsurface (dead at
fractional scale) or a capturable snapshot (dead — `current_image()` empty).
The remaining lever: **remove the UI's render nodes from the scene when nothing
in the UI is visible** (which is most of playback — controls auto-hide, the
overlay is fully transparent, and subtitles are mpv-rendered).

**The mechanism is already validated on the live app**, by the *failed*
CachedOverlay experiment read correctly: replacing the WebView's real node with
a blank texture dropped live CPU **54% → 6%** (`BENCHMARKS.md` §2). The black
screen was the capture bug; the CPU drop is proof that **excluding the WebView's
node from the scene reaches the video-only floor**. `FreezeOverlay` does exactly
that exclusion — but only while the UI is invisible anyway, and with **no
texture cache**, so the entire invalidation-correctness bug class that killed
CachedOverlay does not exist.

### Why the empty-`snapshot()` wrapper is the one correct exclusion mechanism

- GTK collects render nodes by recursing widget `snapshot()`. A wrapper that
  early-returns contributes **zero nodes** → the renderer never touches the
  WebView subtree → per-frame cost is the video-only floor, at any scale, on any
  driver.
- The WebView stays **mapped** → WebKit still believes it is visible: no Page
  Visibility flip, timers/rAF run, its frame stays warm → instant, flicker-free
  unfreeze.
- The WebView keeps **keyboard focus** (focus is tied to mapped/visible state,
  which is untouched) → Space/arrows work while frozen.
- GTK4 **picking is geometry-based, not render-node-based** → the invisible
  overlay keeps receiving pointer/scroll/key events exactly like today's
  transparent overlay. Input path: unchanged.
- `set_opacity(0.0)` is a near-equivalent (also stays mapped) but its node-drop
  guarantee lives in GTK internals; the empty `snapshot()` is deterministic by
  construction and the scaffold already exists in `src/app/cached_overlay/`.

## Signal source (build item, not a diagnostic)

**No controls-visibility signal exists today.** `src/app/ipc/event.rs` /
`src/app/ipc/request.rs` contain no such event; hosted stremio-web never sends
one. It must be synthesized with infrastructure the shell already owns:

- The shell loads the **official hosted production stremio-web**
  (`src/config.rs:8` → `web.stremio.com` proxied via :11470), injected-into via
  `PRELOAD_SCRIPT` (`src/app/ipc/preload.js`, wired at `src/app/imp.rs:79-80`).
  There is no pinned stremio-web checkout — **DOM anchors WILL drift with
  production**. Therefore the observer is designed **fail-visible with a
  heartbeat**: selector drift degrades to today's CPU behaviour, never to a
  hidden UI.
- Observer (appended to `preload.js`): reports one boolean `uiVisible` =
  "**any** player chrome visible" — controls shown ⋁ buffering spinner ⋁
  next-episode/binge prompt ⋁ toast ⋁ not on the player route ⋁ player root not
  found (**default `true`**). `MutationObserver` on the player-root class
  attribute + the exception selectors, re-armed on route change; posts on
  change **and a 5 s heartbeat**.
- Transport: a **dedicated** script-message handler `shell_ui`
  (`window.webkit.messageHandlers.shell_ui.postMessage(...)`), registered in
  `src/app/webview/mod.rs` beside `connect_ipc`
  (`register_script_message_handler("shell_ui", None)` + a new
  `connect_ui_visibility<T: Fn(bool)>`). Do **not** overload the stremio type-6
  protocol in `request.rs` — this is shell-private plumbing.
- Timer-driven web content (binge prompt, toasts) appears **without user
  input** — this is why the signal must come from the DOM, not any shell-side
  idle timer.
- Free corroboration (diagnostic only): stremio-web sets `cursor: none` when
  immersed and WebKitGTK maps CSS cursor to the widget cursor → `notify::cursor`
  on the inner `webkit::WebView`. Cannot see toasts/prompts; never primary.

## Freeze policy

```
frozen = playing                       // playback-started..playback-ended (src/app/imp.rs:88-110)
      && !ui_visible                   // shell_ui observer report (fail-visible + heartbeat)
      && (now - last_local_input) > DEBOUNCE   // 500 ms stale-"hidden" guard
```

- **Eager local unfreeze:** capture-phase `EventControllerMotion` +
  `EventControllerKey` + `EventControllerScroll` on the Window unfreeze
  **immediately** on any input — no web round-trip — and the event still reaches
  the WebView (it was never unmapped), which wakes the web controls in the same
  frame. The web report only ever *freezes*; local input only ever *unfreezes*.
- **Debounce (race fix):** a stale "hidden" report can arrive just after local
  input; ignore "hidden" younger than 500 ms since last local input. `shell_ui`
  messages are FIFO, so state always converges.
- **Heartbeat watchdog:** if playing and no observer message for >2 heartbeats,
  unfreeze and stay unfrozen (selector drift → safe degradation).
- **Belt-and-braces (optional):** the shell already sees `pause`,
  `paused-for-cache`, `buffering`, `seeking`, `eof-reached` via
  `connect_mpv_property_change` (`src/app/imp.rs:112`, whitelist in
  `src/app/video/config.rs`) — may additionally unfreeze on
  `paused-for-cache`. Not required for correctness: buffering shows the spinner
  → observer reports visible; and a paused/buffering video queues no renders, so
  a live overlay is nearly free then anyway.
- Non-player routes: `playing == false` → never frozen (no per-frame video
  invalidation there anyway).

## Subtitles (the gate that keeps A sufficient)

Both subtitle paths are **mpv-rendered**: embedded tracks via `sid`/`track-list`
(`src/app/video/config.rs`), addon (OpenSubtitles) subs via `sub-add` commands
issued by stremio-web through the existing `mpv-command` channel. The shell has
**no web-side subtitle surface**. Step 1a formally confirms this on the live
app; it is the only fork that could ever activate the Appendix.

---

## Ordered task list

1. **Gate check + DOM anchors (~½ day, Nvidia box, `dev_mode` inspector on).**
   a. Play embedded-sub and OpenSubtitles-addon-sub content; hide the player
      chrome in the inspector; confirm subs persist (mpv-rendered). If addon
      subs vanish → record, and schedule the Appendix. *(Expected: they persist.)*
   b. Pin DOM anchors from the live inspector: player root + auto-hide class,
      spinner, binge prompt, toast container. Record in DEVLOG §18, marked
      **version-fragile** (hosted web UI).
   c. Corroboration check: `notify::cursor` flips to `none` on immersion.
   d. (Parallel, non-blocking, contingency-only) Probe
      `webkit6::WebView::snapshot(Visible, TRANSPARENT_BACKGROUND, …)`
      (binding verified: `webkit6-0.6.1/src/auto/web_view.rs:926`) → PNG.
2. **`FreezeOverlay` widget** — new `src/app/freeze_overlay/{mod,imp}.rs`,
   derived from `src/app/cached_overlay/`: keep the `measure`/`size_allocate`
   child delegation verbatim; delete `paintable`/`cached`/`dirty`; add
   `frozen: Cell<bool>`;
   `fn snapshot(&self, s) { if self.frozen.get() { return; } self.parent_snapshot(s) }`;
   `set_frozen(bool)` → `queue_draw()` on change. Port
   `cached_overlay/tests.rs` for measure/allocate. Then **retire
   `cached_overlay` and the `STREMIO_CACHED_OVERLAY` branch**.
3. **Wire into the window** — `src/app/window/mod.rs:set_overlay`:
   `overlay.add_overlay(&FreezeOverlay::new(&graphics_offload(widget)))`
   (keep the offload wrapper: no-op at 170%, harmless, preserves any
   integer-scale benefit). Add `Window::set_ui_frozen(bool)`.
4. **Controlled proof** — add `BENCH_MODE=frozen` to `examples/overlay_bench.rs`
   (freeze the cairo overlay). Expect ≈ `none` row. Do this **before** the IPC
   work.
5. **Observer** — extend `src/app/ipc/preload.js` (fail-visible `uiVisible`,
   5 s heartbeat, `shell_ui` handler); register handler + add
   `connect_ui_visibility` in `src/app/webview/mod.rs`.
6. **Freeze controller** — `src/app/imp.rs:activate`: state
   (playing / ui_visible / last_input), capture-phase motion+key+scroll
   controllers on the Window, 500 ms debounce, heartbeat watchdog,
   `playback-started/ended` gating; drive `window.set_ui_frozen`.
7. **Live validation** (Nvidia @170%, 4K; screenshot-verify pixels every time —
   the CachedOverlay trap's antidote):
   - Idle controls → CPU within the success criterion; UI **provably reappears**
     on mousemove, no flicker; Space/arrows and scroll-volume work *while frozen*.
   - Forced rebuffer (throttle server.js) → spinner visible; embedded + addon
     subs render throughout; binge prompt appears unfrozen; fullscreen toggle +
     resize while frozen; integer-scale pass; GTK Inspector recorder shows zero
     WebView nodes while frozen.
   - Wake latency: timestamp input-event → unfreeze `queue_draw`; target <1 frame.
8. **Matrix + docs** — AMD/Intel rows in `BENCHMARKS.md` (same invocations);
   ADR-0005 recording this decision and correcting ADR-0003's "offloaded
   subsurface" wording to "`current_image()` rasterizes empty"; DEVLOG §18.

## Success criteria (all must hold, all three GPUs, at 170% scale)

- CPU during 4K playback, controls idle, **within ~1.3× of the `none`
  (video-only) floor measured in the same session on the same box** — the floor
  is resolution/box-dependent, so the criterion is relative, not an absolute %.
- UI fully interactive; reappears with no perceptible flicker/latency; keyboard
  shortcuts work while frozen.
- Subtitles (embedded **and** addon), seekbar, spinner, binge prompt, toasts all
  correct during playback.
- Observer failure (selector drift / heartbeat loss) degrades to today's CPU
  behaviour — never to hidden UI.
- No regression at integer scale or on AMD/Intel.

## Measurement method

- **Controlled:** `examples/overlay_bench.rs` `BENCH_MODE=frozen`, same
  `/proc/self/stat` CPU + fps harness.
- **Live A/B:** `LC_NUMERIC=C SERVER_PATH=$PWD/data/server.js GSK_RENDERER=gl …`,
  CPU via `ps`/`top`, plus a screenshot check that the UI actually renders.
- **Per-platform matrix:** `BENCHMARKS.md` — Nvidia here, AMD/Intel on the other
  laptop, same invocation.

---

## Appendix — Contingency B (ONLY if Step 1a shows web-rendered subtitles)

Trigger: **addon subtitles proven web-rendered** (a persistent animated web
element over advancing video). Spinner/binge/toast do **not** trigger this —
they are transient and handled by keeping the overlay live during them.

Corrected design (the original "offscreen WebView + forwarded events" is ruled
out — see table):

- WebView stays **in-scene, mapped, full-size, permanently frozen** via the same
  `FreezeOverlay` → real input flows natively; only its pixels are suppressed.
- Capture via `webkit6::WebView::snapshot(SnapshotRegion::Visible,
  SnapshotOptions::TRANSPARENT_BACKGROUND, …)` → `gdk::Texture` → GL upload;
  blended after `render_context.render(...)` in `src/app/video/imp.rs:render`.
- Trigger captures from the `shell_ui` observer ("dirty" reports), **not**
  `WidgetPaintable::invalidate-contents` (unverified semantics on the live
  WebView); throttle ≤15 Hz — a full-view 4K RGBA snapshot is ~33 MB through
  async web-process IPC, so unthrottled capture rebuilds the CPU problem.
- Colour: current pipeline is SDR/sRGB end-to-end (GL renderer; mpv renders into
  the GTK-provided fbo) — blend premultiplied-sRGB in the same fbo pass. Revisit
  if HDR passthrough ever lands (one more reason to prefer A).
- Known costs: visible-UI framerate capped by the throttle (seekbar-drag
  stutter risk) — acceptable only because the trigger means A alone cannot
  satisfy constraint 2.

## Environment (reference)

GTK 4.22.4 · libadwaita 1.9.0 · WebKitGTK 2.52.3 · mpv/libmpv 2.5.0 · Nvidia
580.159.03 (GTX 1060) · KDE Plasma/KWin Wayland · 3840×2160 @ 170% · flatpak on
`org.gnome.Platform` 50 · webkit6 crate 0.6.1.
