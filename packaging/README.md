# Packaging

## Which Ubuntu releases are targeted, and why

`releases.json` is the single source of truth. Both the CI matrix
(`.github/workflows/release.yml`, via `fromJSON`) and the local test harness
(`test-deb.sh`) read it, so they cannot drift apart. Adding a release is one
entry in that file.

A `.deb` is built **once per target release, inside a container of that
release**. This is not incidental: it is what lets `cargo-deb`'s `$auto`
dependency resolution run `dpkg-shlibdeps` against the exact libraries the
binary will link at runtime, producing correct package names and minimum
versions for that release. A single `.deb` built on one release and shipped to
another would get its dependency floors wrong.

The obvious shortcut — build on the oldest supported release and rely on
forward compatibility — **does not hold here**. Ubuntu backports unevenly:
noble (24.04) ships WebKitGTK 2.52.3 while the newer plucky (25.04) shipped only
2.50.4. Library versions are not monotonic across releases, so "oldest" is not a
safe lower bound.

### Current targets

| release | status | GTK | libadwaita | WebKitGTK |
| --- | --- | --- | --- | --- |
| noble 24.04 LTS | supported to 2029-05 | 4.14.5 | 1.5.0 | 2.52.3 |
| resolute 26.04 LTS | supported to 2031-05 | 4.22.4 | 1.9.0 | 2.52.3 |
| stonking 26.10 | interim (releases 2026-10-15) | 4.23.2 | 1.9.1 | 2.52.4 |

Each target selects a Cargo feature set (`api-4_14`, `api-4_22`) pinning the
GTK/libadwaita/WebKitGTK API levels to what that release actually ships.
Building with a higher API level than the target ships produces a binary that
fails at runtime on that target.

The sets are named after API levels, not distributions: the crate has no opinion
about Ubuntu, and the Flatpak builds with `default` features and should not
appear to depend on an Ubuntu release. `releases.json` owns the
release → API-set mapping, so several releases can share a set (Ubuntu 26.10
ships GTK 4.23 and would reuse `api-4_22`) and a non-Ubuntu target can be added
later without renaming anything.

### Deliberately excluded

**jammy (22.04 LTS)** — supported until 2027-06, but the crate cannot be built
there at all. `webkit6` references `gtk::Accessible` in its generated bindings
with no version cfg guard, and that type requires GTK feature `v4_10`+. jammy
ships GTK 4.6.9, so the build fails inside the dependency:

```
error[E0425]: cannot find type `Accessible` in crate `gtk`
  --> webkit6-0.6.1/src/auto/web_view.rs:27
```

No feature selection avoids this — it is in a dependency, not in our code.
(Note jammy *does* ship `libwebkitgtk-6.0-dev` 2.50.4; the blocker is GTK, not
WebKit.) jammy users are served by the Flatpak, which bundles its own runtime.

**Interim releases** — as of 2026-07 every Ubuntu interim release is EOL:
oracular (24.10) EOL 2025-07 and its archive is gone entirely, plucky (25.04)
EOL 2026-01, questing (25.10) EOL 2026-07-09. There is currently no supported
interim release to target: 26.04 LTS shipped in April and 26.10 (stonking) does
not release until 2026-10-15. stonking already builds cleanly (GTK 4.23.2,
libadwaita 1.9.1, WebKitGTK 2.52.4) and can be added to `releases.json` when it
ships.

Re-check with `ubuntu-distro-info --supported` — do not assume.

## NVIDIA, decode, and the playback-CPU fix

The app cuts playback CPU by asking mpv for **zero-copy** hardware decoding
(`src/app/video/mod.rs`): `auto-safe` (VAAPI dmabuf) on Mesa (AMD/Intel), and
`nvdec` (CUDA interop) on the NVIDIA proprietary driver. Unlike the Flatpak,
the `.deb` links the **system** libmpv, GTK and WebKitGTK, so whether the win
actually lands depends on how the distro built those libraries. Two things
matter on NVIDIA:

### 1. libmpv must be built with CUDA interop (else nvdec → copy-back)

`hwdec=nvdec` silently degrades to copy-back (every 4K/10-bit frame round-trips
through system RAM — back to ~100% of a core) unless the distro's libmpv was
built `--enable-cuda-hwaccel --enable-cuda-interop` **and** its ffmpeg was built
`--enable-ffnvcodec`. libmpv links libavcodec dynamically, so these are two
independent halves that must both hold.

Every release in `releases.json` carries an **`expect_nvdec`** flag recording
whether it ships both halves (audited in a fresh container of that release; see
DEB-BUILD-PLAN.md). All current targets — noble 24.04, resolute 26.04, stonking
26.10 — pass. `packaging/verify-deb.sh` **TEST 8** re-checks this against the
libraries the built `.deb` pulls in and **hard-fails** any target flagged
`expect_nvdec: true` that has lost the capability (a regression guard); on
unflagged targets it only warns. The detector is `packaging/check-nvdec.sh`
(unit-tested by `check-nvdec.test.sh`).

To confirm what a running instance actually chose, the app sets `terminal=yes`,
so mpv logs `Using hardware decoding (nvdec)` at startup — `nvdec` is zero-copy,
anything ending `-copy` is copy-back.

### 2. WebKitGTK ≥ 2.52.4 for a smooth UI on NVIDIA

On the NVIDIA proprietary driver, **WebKitGTK < 2.52.4 lags and shows rendering
artifacts** in the `.deb` (the Flatpak bundles its own WebKit and is unaffected).
This is a WebKitGTK version bug fixed upstream in 2.52.4/2.52.5, not a build-flag
or a Stremio issue, and it is **independent of decode** — `nvdec` still works.

- **noble 24.04 / resolute 26.04** ship WebKitGTK **2.52.3** today → affected.
  The fix arrives via Ubuntu's WebKit microrelease SRUs; until then, NVIDIA users
  who want a smooth UI should use the Flatpak.
- **stonking 26.10** already ships **2.52.4** → unaffected.

This is **not** a hard `Depends`: a global `libwebkitgtk-6.0-4 (>= 2.52.4)` would
make the `.deb` uninstallable for *everyone* (AMD/Intel included) while the
archive still ships 2.52.3, to fix an NVIDIA-only problem. Instead the launcher
(`data/stremio.sh`) prints a one-line heads-up at startup when it detects NVIDIA
(`/dev/nvidia0`) **and** a system WebKitGTK below 2.52.4. The hard versioned
floor gets added only once 2.52.4+ is in every target's `-updates` pocket (see
DEB-BUILD-PLAN.md step 5).

### AV1 on pre-Ampere NVIDIA

AV1 **hardware** decode starts at **Ampere (RTX 30)**. Turing (RTX 20 / GTX 16),
Pascal (GTX 10) and older have no AV1 decoder, so AV1 content falls back to
software decode (heavy at 4K) regardless of `nvdec`, the `.deb`, or the Flatpak —
a GPU limitation, not a bug. `nvdec` zero-copy still applies to H.264/HEVC.

### Overriding the decode mode (support/debug)

`STREMIO_HWDEC` overrides the auto-selected mode without a rebuild, e.g.
`STREMIO_HWDEC=auto-copy` (force copy-back, for an A/B comparison),
`STREMIO_HWDEC=nvdec`, `STREMIO_HWDEC=auto-safe`, or `STREMIO_HWDEC=no`
(software). Combined with mpv's `Using hardware decoding (...)` log line, this is
the quickest way to confirm a suspected copy-back regression on a user's box.

## Testing locally

Requires Docker. Mirrors exactly what CI does:

```bash
./packaging/test-deb.sh            # every release in releases.json
./packaging/test-deb.sh noble      # just one
```

Per release it builds the `.deb` in a container of that release, then in a
**fresh** container of the same release verifies that:

1. `apt-get install ./x.deb` resolves every dependency (catches bad `Depends`),
2. the installed binary executes,
3. the expected files landed at the expected paths,
4. the installed libmpv/ffmpeg can do `nvdec` zero-copy (TEST 8; a hard fail on
   targets flagged `expect_nvdec`, so an NVIDIA copy-back regression cannot ship).

Step 1 is the one that matters most: a `.deb` whose `Depends` name packages that
do not exist is the failure mode this packaging has actually hit before.

## Getting a CI-built .deb (without Docker or a release)

To get correct, smoke-tested debs without building locally — and without cutting
a release — trigger the Release workflow manually:

```bash
gh workflow run release.yml --repo <owner>/stremio-linux-shell --ref <branch>
gh run download <run-id>          # after it finishes; debs are per-target artifacts
```

It builds one `.deb` per `releases.json` target, runs the same `verify-deb.sh`
checks (TEST 1–8), and uploads each as a run artifact. The release-upload and
flatpak jobs are skipped for manual runs — they only fire on a published release.

Prefer this over a hand-built deb: a stale or wrong-branch `.deb` silently misses
recent fixes (it is exactly how a build without the playback-CPU fix once got
installed and looked like a regression).

## Versioning

Packages are versioned `<upstream>-1~ubuntu<release>`, e.g.
`1.1.2-1~ubuntu24.04`. The `~` sorts *before* the plain version in Debian's
ordering, and `~ubuntu24.04` sorts before `~ubuntu26.04`, so a user upgrading
from 24.04 to 26.04 correctly receives the newer package rather than a downgrade
or a version conflict. This is why the release is encoded in the version rather
than bolted onto the filename after the architecture, which would break the
`name_version_arch.deb` convention that apt and dpkg rely on.
