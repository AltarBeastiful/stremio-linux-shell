<div align="center">

![Stremio icon](data/icons/com.stremio.Stremio.svg "Stremio icon")

# Stremio on Linux 
Client for Stremio on Linux using [`gtk4`](https://docs.gtk.org/gtk4/) + [`libadwaita`](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.8/) + [`WebKitGTK`](https://webkitgtk.org/) + [`libmpv`](https://github.com/mpv-player/mpv/blob/master/DOCS/man/libmpv.rst)

<img src="data/screenshots/screenshot1.png" alrt="Screenshot" width="800" />

</div>

## Installation

```bash
flatpak install com.stremio.Stremio
```

## Development

```bash
git clone --recurse-submodules https://github.com/Stremio/stremio-linux-shell
```

#### Fedora
```bash
dnf install gtk4-devel libadwaita-devel webkitgtk6.0-devel mpv-devel libepoxy-devel nodejs flatpak-builder
dnf install python3 && python3 -m pip install aiohttp toml # Needed for Flatpak build
```

```bash
cargo run --release # RUST_LOG=debug to print debug logs
```

#### Debian-based (Ubuntu, etc.)
```bash
apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev libmpv-dev gettext nodejs flatpak-builder
apt install python3 python3-aiohttp python3-toml elfutils # Needed for Flatpak build
```

```bash
cargo run --release # RUST_LOG=debug to print debug logs
```

#### Flatpak
```bash
flatpak install -y \
    org.gnome.Sdk//50 \
    org.gnome.Platform//50 \
    org.freedesktop.Sdk.Extension.rust-stable//25.08 \
    org.freedesktop.Platform.codecs-extra//25.08-extra \
    org.freedesktop.Platform.VAAPI.Intel//25.08
```

```bash
./flatpak/build.sh
flatpak install ./flatpak/com.stremio.Stremio.Devel.flatpak
flatpak run com.stremio.Stremio.Devel
```

#### Debian package (.deb)
A `.deb` package is built and attached to every [release](https://github.com/Stremio/stremio-linux-shell/releases),
for each natively supported Ubuntu release:

| Ubuntu release       | Codename   | Artifact suffix |
|-----------------------|-----------|------------------|
| 24.04 LTS             | `noble`   | `-noble.deb`     |
| devel (pre-26.04 LTS) | `resolute`| `-resolute.deb`  |

Each `.deb` is built inside a container matching its target release, so runtime dependencies
(`libgtk-4-1`, `libadwaita-1-0`, `libwebkitgtk-6.0-4`, `libmpv2`, etc.) are resolved automatically from the
libraries actually linked into that build, matching the package names and minimum versions available on
that release. Install the `.deb` matching your Ubuntu version.

22.04 LTS (`jammy`) is **not** built as a native `.deb`: it only ships GTK4 4.6.9, while the `webkit6` Rust
bindings used by this project require GTK4 >= 4.10 to compile, and there is no verified PPA that backports a
newer GTK4 to jammy. Users on jammy or any other unsupported release should use the Flatpak package instead,
which bundles its own GNOME runtime and is unaffected by the host's system library versions.

To build a `.deb` locally, pick the Cargo feature matching your distribution's GTK4/libadwaita/WebKitGTK
versions (see `[features]` in `Cargo.toml`):

```bash
cargo install cargo-deb --locked
cargo deb -- --no-default-features --features ubuntu-noble # or ubuntu-resolute
sudo apt install ./target/debian/stremio_*_amd64.deb
```
