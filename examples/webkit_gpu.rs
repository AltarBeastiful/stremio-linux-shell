//! Probes what makes WebKitGTK's WebView snapshot to a GPU *texture* node vs a
//! software `GskCairoNode` — the difference that pins a CPU core on Nvidia when
//! the (transparent) UI is composited over the video every frame.
//!
//! Env knobs:
//!   WK_URL     = uri to load (default webkit://gpu/stdout)
//!   WK_POLICY  = always | ondemand | never   (hardware-acceleration-policy)
//!   WK_BG      = transparent | opaque         (webview background)
//!
//! Run under `GDK_DEBUG=offload` (and GDK_BACKEND=x11 for integer scale) to see
//! whether the offloaded webview is a texture (offloads) or "Only textures
//! supported (found GskCairoNode)".
//!
//!   GDK_BACKEND=x11 GDK_DEBUG=offload WK_POLICY=always WK_BG=transparent \
//!     cargo run --example webkit_gpu

use gtk::{gdk::RGBA, glib, prelude::*};
use webkit::prelude::*;

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("com.stremio.WebkitGpuProbe")
        .build();

    app.connect_activate(|app| {
        let window = gtk::ApplicationWindow::new(app);
        window.set_default_size(1024, 768);

        let webview = webkit::WebView::new();

        if let Some(settings) = WebViewExt::settings(&webview) {
            let policy = match std::env::var("WK_POLICY").as_deref() {
                Ok("never") => webkit::HardwareAccelerationPolicy::Never,
                _ => webkit::HardwareAccelerationPolicy::Always,
            };
            settings.set_hardware_acceleration_policy(policy);
            eprintln!("PROBE policy={policy:?}");
        }

        if std::env::var("WK_BG").as_deref() != Ok("opaque") {
            webview.set_background_color(&RGBA::new(0.0, 0.0, 0.0, 0.0));
            eprintln!("PROBE bg=transparent");
        } else {
            eprintln!("PROBE bg=opaque");
        }

        let url = std::env::var("WK_URL")
            .unwrap_or_else(|_| "webkit://gpu/stdout".to_string());
        webview.load_uri(&url);

        // Offload wrapper so GDK_DEBUG=offload reports the snapshot node type.
        let offload = gtk::GraphicsOffload::builder()
            .child(&webview)
            .hexpand(true)
            .vexpand(true)
            .build();
        window.set_child(Some(&offload));
        window.present();

        let app = app.clone();
        glib::timeout_add_seconds_local_once(6, move || app.quit());
    });

    app.run()
}
