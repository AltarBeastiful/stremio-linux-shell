//! GPU-based selection of the GSK renderer.
//!
//! GTK 4.14+ defaults to the Vulkan GSK renderer. This shell draws the video
//! into an OpenGL `GtkGLArea` underlay, and compositing that GL texture through
//! the Vulkan renderer forces a per-frame GL->Vulkan bridge that dominates
//! playback CPU even when the video is decoded on the GPU:
//!
//!   - on the proprietary Nvidia driver the bridge is a full readback (upstream
//!     works around this in data/stremio.sh for /dev/nvidia0), and
//!   - on Mesa it is a costly texture import: measured on this project's AMD
//!     test machine at ~24% CPU on the Vulkan renderer versus ~8% on the GL one
//!     for 1080p playback.
//!
//! The GL renderer composites the GLArea natively, with no such bridge, on every
//! driver -- so we prefer it whenever a GPU is present. That covers AMD, Intel
//! and nouveau alike (all Mesa, all paying the same import), not just the Nvidia
//! case upstream special-cased. Only a display with no GPU (software rendering)
//! keeps GTK's default, and a user can always override through GSK_RENDERER.
//! Deciding this in the binary rather than only in data/stremio.sh means every
//! package (deb, Flatpak, source) behaves the same.

use std::{fs, path::Path};

/// The GSK renderer to force for this machine, or `None` to leave GTK's default
/// in place. `gl` and `opengl` name the same renderer on GTK 4.14-4.22; `gl` is
/// the spelling `GSK_RENDERER=help` lists.
pub fn preferred_gsk_renderer() -> Option<&'static str> {
    has_gpu().then_some("gl")
}

/// Whether a real GPU is present, which is what makes GTK default to the Vulkan
/// renderer. /dev/nvidia0 is the proprietary Nvidia driver; a `cardN` DRM node
/// covers every Mesa driver (amdgpu, i915, nouveau, ...).
fn has_gpu() -> bool {
    Path::new("/dev/nvidia0").exists() || has_drm_card()
}

/// True when /sys/class/drm exposes at least one primary `cardN` node.
fn has_drm_card() -> bool {
    fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| is_card_node(&entry.file_name().to_string_lossy()))
}

/// True for a primary DRM node name: `card` followed by digits only, so `card1`
/// matches but the `card1-DP-1` connectors and `renderD*` nodes do not.
fn is_card_node(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
}
