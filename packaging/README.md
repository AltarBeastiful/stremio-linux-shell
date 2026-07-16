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
3. the expected files landed at the expected paths.

Step 1 is the one that matters most: a `.deb` whose `Depends` name packages that
do not exist is the failure mode this packaging has actually hit before.

## Versioning

Packages are versioned `<upstream>-1~ubuntu<release>`, e.g.
`1.1.2-1~ubuntu24.04`. The `~` sorts *before* the plain version in Debian's
ordering, and `~ubuntu24.04` sorts before `~ubuntu26.04`, so a user upgrading
from 24.04 to 26.04 correctly receives the newer package rather than a downgrade
or a version conflict. This is why the release is encoded in the version rather
than bolted onto the filename after the architecture, which would break the
`name_version_arch.deb` convention that apt and dpkg rely on.
