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
A `.deb` is built and attached to every
[release](https://github.com/Stremio/stremio-linux-shell/releases), one per supported Ubuntu
release — currently **24.04 LTS (noble)** and **26.04 LTS (resolute)**. Pick the one matching your
release: they differ in which GTK/libadwaita/WebKitGTK versions they link against, and installing
the wrong one will fail dependency resolution rather than misbehave silently.

Ubuntu 22.04 has no `.deb` and cannot have one — see
[`packaging/README.md`](packaging/README.md). Use the Flatpak there.

To build one locally for the release you are on:

```bash
cargo install cargo-deb --locked
# on 24.04; use --features api-4_22 and 1~ubuntu26.04 on 26.04
cargo deb --deb-revision "1~ubuntu24.04" -- --no-default-features --features api-4_14
sudo apt install ./target/debian/stremio_*.deb
```

The feature set must match the GTK your release ships (`api-4_14` for 24.04, `api-4_22` for 26.04;
see `[features]` in `Cargo.toml` and the mapping in
[`packaging/releases.json`](packaging/releases.json)). Building with a newer set than your system
ships compiles fine and then fails at runtime.

To build and verify every target the way CI does — each `.deb` built in a container of its release,
then installed into a *fresh* one to prove its dependencies actually resolve:

```bash
./packaging/test-deb.sh          # all releases
./packaging/test-deb.sh noble    # just one
```
