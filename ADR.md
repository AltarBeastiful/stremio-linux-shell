# Architecture Decision Records

Records of the architecture decisions behind the playback-CPU fix and the
multi-distro Debian packaging. Narrative and measurements live in
[`DEVLOG.md`](./DEVLOG.md); this file states the decisions, why they hold, and
what they cost.

Format: lightweight [MADR](https://adr.github.io/madr/). Status is per-record.

---

## ADR-0001 — Select the GSK renderer from GPU presence, in the binary

**Status:** Accepted, in validation (broad-GPU behaviour under test via the
`v1.1.2-cpu-deb-test2` release).

### Context

- GTK 4.14+ defaults to the **Vulkan** GSK renderer where Vulkan is available.
- The shell draws video into an OpenGL `GtkGLArea`. Compositing that GL texture
  through the Vulkan renderer forces a per-frame **GL → Vulkan bridge** that
  dominates playback CPU even with hardware decode — measured ~24% vs ~7.8% for
  the GL renderer on AMD/Mesa (see DEVLOG §1).
- The GL renderer composites a GL `GLArea` **natively**, with no bridge, so for
  this specific workload it cannot be slower than Vulkan on any driver.
- Upstream already worked around this, but only for the proprietary Nvidia
  driver (`/dev/nvidia0`) and only in the launcher script `data/stremio.sh`
  (commit `638b5af`). Upstream PR #69 (VA-API Wayland decode) is unrelated to the
  compositor renderer.

### Decision

Choose the renderer in the **binary**, before GTK initialises:

```mermaid
flowchart TD
  A["main() start"] --> B{"GSK_RENDERER already set?"}
  B -->|yes| R["respect the user / wrapper value"]
  B -->|no| C{"GPU present?<br/>/dev/nvidia0 OR /sys/class/drm/cardN"}
  C -->|yes| G["set GSK_RENDERER=gl"]
  C -->|no| D["leave GTK's default (software)"]
```

Implemented as `gpu::preferred_gsk_renderer()` (`src/gpu.rs`, std-only) returning
`Some("gl")` when a GPU is present, wired into `main.rs`.

Three sub-decisions:

1. **In the binary, not the shell wrapper.** `main.rs` runs in *every* package
   (deb, Flatpak, run-from-source); the wrapper is Flatpak/deb-only and Nvidia-only.
   The binary is the single place that fixes all of them, and it composes with
   the wrapper (which sets `opengl` for Nvidia first — respected by the `is_none`
   guard; `opengl` and `gl` are the same renderer).

2. **Gate on GPU presence, not GPU vendor.** The bridge cost is structural to
   "GL GLArea through the Vulkan compositor" and shared across Mesa drivers
   (AMD, Intel, nouveau); the GL renderer has no downside for this workload. A
   vendor allow-list would exclude drivers that pay the same cost on the excuse
   of "unmeasured". Only true software rendering (no GPU) keeps GTK's default.

3. **Value `gl`, and never override the user.** `gl` == `opengl` on GTK
   4.14–4.22 and is the canonical `GSK_RENDERER=help` name. An explicit
   `GSK_RENDERER` always wins.

### Consequences

**Positive**

- The CPU fix applies across GPU vendors and across all package formats.
- It also fixes run-from-source on Nvidia, which the wrapper-only approach misses.
- The renderer logic is isolated, documented, and testable in std-only code.

**Negative / risks**

- It **overrides GTK's chosen default (Vulkan)** for all GPU users — deliberate,
  but broader than upstream's Nvidia-only scope.
- The broad default is **verified only on AMD**; Intel and nouveau are reasoned
  from the shared Mesa mechanism, not measured. The residual risk is a
  driver-specific GL *visual* quirk, not CPU. Mitigated by (a) the `GSK_RENDERER`
  escape hatch and (b) the `v1.1.2-cpu-deb-test2` release for cross-GPU testing.
- If upstreamed, this changes the **official Flatpak** for all Linux users, so
  the blast radius warrants the cross-GPU testing before promotion.

### Alternatives considered

- **Unconditional `gl` (v1).** Simplest, but forces GL with no GPU and reads as a
  blunt override. GPU-present is barely more code and documents intent.
- **Nvidia-only, in the wrapper (upstream).** Under-inclusive: never fires on
  AMD (no `/dev/nvidia0`), so it would not have fixed the machine that motivated
  this work.
- **Vendor allow-list (Nvidia + AMD, v2).** Excludes Intel/nouveau, which share
  the mechanism; rejected as false conservatism.
- **Architectural fix — dmabuf paintable + `GtkGraphicsOffload`.** Feed mpv output
  as a dmabuf paintable and let GTK offload it to a Wayland subsurface, bypassing
  GSK compositing (and this whole renderer question) entirely. The correct
  long-term direction, but a rework of `src/app/video/imp.rs` and out of scope for
  a CPU fix. Recorded as future work.

---

## ADR-0002 — One `.deb` per Ubuntu release, built and verified in release containers

**Status:** Accepted.

### Context

- The crate pins GTK/libadwaita/WebKitGTK API levels via Cargo feature sets; a
  single build cannot target Ubuntu releases that ship different library
  versions.
- cargo-deb's `$auto` derives `Depends` names and floors from what the binary
  links — but only correctly when the matching `-dev` packages are present.
  Otherwise `dpkg-shlibdeps` invents a name from the SONAME (`libgtk-4` instead
  of the real `libgtk-4-1`) and the deb builds cleanly yet installs nowhere.
- A hand-pinned `Depends` (`libgtk-4-1 (>= 4.22) …`) is wrong for every release
  but one and made the 24.04 deb uninstallable.

### Decision

- **Targets are data:** `packaging/releases.json` maps each Ubuntu release to a
  feature set; CI and the local harness both read it, so they cannot drift.
- **Build each deb in a container of its target release**, so `$auto` resolves
  dependencies against that release's actual libraries. Encode the release in the
  Debian revision (`1~ubuntu24.04`) — `~` sorts before any suffix, so 24.04 →
  26.04 is an upgrade, not a phantom downgrade.
- **Verify by installing into a *fresh* container** of the same release (no
  `-dev` packages) before shipping — the only check that catches invented
  dependency names. This is `packaging/verify-deb.sh`, run identically by CI and
  `packaging/test-deb.sh`.
- `Depends = "$auto, nodejs"` (nodejs isn't linked, so `$auto` is blind to it);
  `Recommends = desktop-file-utils, hicolor-icon-theme` (dpkg file triggers for
  URL-scheme + icon registration; the app runs without them).

### Consequences

- Adding an Ubuntu release is one entry in `releases.json`.
- Correct, minimal `Depends` per release; regressions are caught before publish.
- **Ubuntu 22.04 cannot be targeted** at any feature set: `webkit6`'s generated
  bindings reference `gtk::Accessible` (GTK v4_10+), and 22.04 ships GTK 4.6. The
  failure is inside the dependency, so no feature selection avoids it.
- Slightly heavier CI (a container build + a fresh-container verify per release).

---

## ADR-0003 — Decouple the UI overlay's compositing from the video frame clock

**Status:** Proposed. Reworked after the subsurface/offload approach was rejected
(see below). Direction chosen; exact mechanism to be settled by one experiment on
hardware (DEVLOG §16). Motivated by DEVLOG §10–§11 and §16.

### Context

ADR-0001 (`gl` renderer) fixes AMD/Mesa but **does not** fix Nvidia: a full CPU
core stays pinned during playback. Root cause (DEVLOG §11):

- The window is a `GtkOverlay` — video `GtkGLArea` underlay, **transparent**
  WebKitGTK UI overlay on top.
- On the proprietary Nvidia driver the UI snapshots to a software `GskCairoNode`.
  New this round (DEVLOG §16, via `webkit://gpu`): WebKit's
  **hardware-acceleration `Policy` is `never` by default** in this build
  (`webkit6` only exposes `Always`/`Never` — no `OnDemand`), so WebKit does no
  accelerated compositing at all. On Mesa the UI still ends up a GPU texture (the
  Skia CPU raster is uploaded cheaply); on Nvidia the whole thing stays on the CPU.
- Because that software surface **overlaps** the video, and the video changes
  every frame, GSK re-processes the UI on the CPU **60×/s** even though the UI
  itself is unchanged. That per-frame re-processing of an *unchanging* overlay is
  the waste — and it is a waste on every driver, just cheap enough to ignore on
  Mesa.

**Why the obvious fix is rejected.** Putting the video on its own Wayland
subsurface (`GtkGraphicsOffload` + a dmabuf `GdkPaintable`) *would* bypass GSK —
but [GTK declines to offload at fractional
scales](https://blog.gtk.org/2024/04/17/graphics-offload-revisited/), and
fractional scaling is common enough that shipping a fix that silently does nothing
for those users is unacceptable. **Any subsurface/offload-based approach is out.**

### Decision (proposed)

Attack the actual waste — re-compositing an **unchanging** UI at video frame rate
— within a single GSK surface (no subsurface, so scale-independent and
all-driver). Two mechanisms are viable; pick per the §16 experiment:

```mermaid
flowchart TD
  V["video GLArea: new frame 60×/s"] --> Q{"does the UI need re-processing?"}
  Q -->|"today"| N["GSK re-processes the software UI every frame (CPU on Nvidia)"]
  Q -->|"(a) cache"| C["composite a cached GdkTexture of the UI (GPU blend); refresh only when the UI actually changes"]
  Q -->|"(b) freeze"| H["skip the UI entirely while it is idle/transparent; re-include on UI activity"]
```

- **(a) Cache the UI as a texture (preferred).** Snapshot the WebView to a
  `GdkTexture` (e.g. via `GtkWidgetPaintable` + an explicit texture cache) and
  composite that cached texture over the video. Refresh it only on the WebView's
  own invalidation (controls show/hide, seekbar tick — ~1/s), not per video frame.
  Per-frame cost becomes a **GPU texture blend at any scale**; WebKit's software
  render happens rarely. Keeps the UI always visible (no UX change).
- **(b) Freeze/skip the idle overlay (lighter).** Set the WebView
  `visible=false` (or otherwise exclude it from the snapshot) while the player UI
  is idle and transparent, and restore it on pointer/key activity. Trivial to
  implement; risk is any web-drawn content that must stay visible during playback
  (needs the §16 subtitle check).
- **Complement, both cases:** set
  `settings.set_hardware_acceleration_policy(Always)` (default is `Never`). It is
  necessary (not sufficient) and reduces the software-render cost on the
  UI-change path for everyone.

### Per-platform outcome

Scale-independent by construction. Expected: **Nvidia** drops from ~1 pinned core
to roughly the video-composite cost (steady playback re-processes the UI ~1/s, not
60/s); **Mesa** gets slightly cheaper too (fewer per-frame uploads); **no platform
regresses** — worst case the UI re-composites as often as today.

### Open question the §16 experiment must answer

Is the per-frame cost the WebView being **re-snapshotted** every frame (WebKit
repainting, or GTK re-snapshotting it because the sibling GLArea invalidates), or
GSK **re-uploading an unchanged** cairo node every frame? If the former, caching
alone won't help — the repaint must be stopped (mechanism (b), or a WebKit fix).
If the latter, caching (a) is the clean fix. One focused-window profile
(webkit://gpu with `Policy=Always`; check whether the WebView's
`invalidate-contents` fires per frame) decides it. The `examples/webkit_gpu.rs`
probe added this round is the harness for that check.

### Consequences

**Positive** — works at every scale and on every driver (the constraint the
subsurface approach failed); no dependency on GTK offload or a WebKit-on-Nvidia
upstream fix; keeps the `gl` default and composes with ADR-0004.

**Negative / risks**
- (a) is real work: a snapshot/texture-cache layer and correct invalidation
  (resize, DPI change, animation). (b) is small but has UX edge cases (subtitles,
  dialogs, buffering spinner drawn by the web UI).
- Still leaves WebKit rendering the UI in software on Nvidia — only its *frequency*
  is fixed. Acceptable: the UI changes rarely; the video does not.

### Alternatives considered (rejected)

- **`GtkGraphicsOffload` / dmabuf video subsurface.** The clean bypass, but GTK
  declines it at fractional scale → breaks a large share of users. **Rejected**
  (this is the reversal from the first draft of this ADR).
- **`hardware-acceleration-policy=Always` alone.** Necessary but insufficient on
  Nvidia — the transparent overlay still snapshots to `GskCairoNode` in testing.
  Kept only as a complement above.
- **Force an integer scale / `GSK_RENDERER=cairo`.** User-hostile / makes
  everything software. Rejected.
- **Wait for upstream WebKit Skia-GPU-on-Nvidia.** Out of the shell's hands and
  no ETA; track it, but do not depend on it for the fix.

---

## ADR-0004 — Scope zero-copy `hwdec` to interops we've verified

**Status:** Proposed. Motivated by DEVLOG §12; needs a playback session to confirm.

### Context

`a7597b7` sets `hwdec=auto-safe` at init and `c1123bc` remaps the web UI's
`hwdec=auto-copy` request to `auto-safe` (`video/mod.rs:131`). `auto-safe` is the
**zero-copy** GPU interop; `auto-copy` copies frames back through system memory.

That change was **verified on VAAPI/Mesa only** ("Using EGL dmabuf interop via
`GL_OES_EGL_image` … Initialized VAAPI"). On the Nvidia box the deb (which forces
`auto-safe`) shows **playback artifacts**; the upstream stable Flatpak — which
sets no `hwdec`, so the web UI's `auto-copy` stands — does **not**, on the same
GTK/mpv versions. So forcing zero-copy on Nvidia's nvdec/CUDA-GL interop is a
**regression**. It is not local to this branch: the same commits ride in upstream
**PR #108**, so merging that PR as-is ships the artifacts to the official Flatpak.

### Decision (proposed)

Do **not** force the zero-copy interop where it is unverified. Prefer the web UI's
`auto-copy` unless the shell has reason to believe zero-copy is sound:

- Apply the `auto-copy → auto-safe` remap (and the init default) **only for the
  VAAPI/Mesa interop** (the one measured), leaving the proprietary Nvidia path on
  the copy-back `auto-copy` the web UI already asks for; **or**
- gate zero-copy on a successful interop probe at render-context creation.

The exact predicate is an implementation detail; the decision is that
`auto-safe`'s scope must match what we've validated, per platform.

### Consequences

- **Positive:** clears the Nvidia artifacts; keeps the measured AMD/Intel zero-copy
  win (that is what `a7597b7`/#107 were about); makes the default honest about
  where it's been proven.
- **Negative:** copy-back on Nvidia costs some CPU vs a (currently broken) zero-copy
  path — an acceptable trade until the Nvidia interop is verified. A per-interop
  predicate is slightly more code than a blanket default.
- **Validation gate:** confirm on the Nvidia box that `auto-copy` clears the
  artifacts before flipping the default (or before PR #108 merges).

### Alternatives considered

- **Keep the blanket `auto-safe`.** Ships known artifacts to Nvidia users. Rejected.
- **Drop hwdec defaults entirely (upstream `main` behaviour).** Loses the verified
  AMD/Intel zero-copy win and reintroduces #107's software-decode cost. Rejected.

---

## Related decisions recorded elsewhere

These were decided during the same work but live in their natural homes:

- **GPU-video-processing / `d3d11vpp` toggle** is Windows-only; the Linux shell
  logs it as unsupported. No change.
- **VLC-in-Flatpak detection** cannot be fixed in this shell — player detection
  is in the closed-source `server.js`. Filed upstream. See DEVLOG §8.
- **`server.js` performance** is I/O-bound with no hotspot; no drop-in backend
  changes playback CPU. See DEVLOG §8.
