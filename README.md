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
A `.deb` package is built and attached to every [release](https://github.com/Stremio/stremio-linux-shell/releases).
The GTK, libadwaita and WebKitGTK runtime dependencies (`libgtk-4-1`, `libadwaita-1-0`,
`libwebkitgtk-6.0-4`) are pinned explicitly in `[package.metadata.deb]`; the remaining transitive
dependencies (`libmpv2`, `libc6`, etc.) are resolved from the libraries linked into the binary via
`$auto`. See the comment in `Cargo.toml` for why the pinned ones cannot be left to `$auto`.

```bash
cargo install cargo-deb --locked
cargo deb # produces target/debian/stremio_<version>_amd64.deb
sudo apt install ./target/debian/stremio_*_amd64.deb
```
