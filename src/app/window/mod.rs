mod imp;

use adw::subclass::prelude::*;
use gtk::{
    Widget, gio,
    glib::{self, object::IsA},
    prelude::{GtkWindowExt, WidgetExt},
};

use crate::app::{Application, freeze_overlay::FreezeOverlay};

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
    @extends gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow, gtk::Widget,
    @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager, gtk::Native, gtk::Root;
}

impl Window {
    pub fn new(application: &Application) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    pub fn set_underlay(&self, widget: &impl IsA<Widget>) {
        let window = self.imp();

        window.overlay.set_child(Some(widget));
    }

    pub fn set_overlay(&self, widget: &impl IsA<Widget>) {
        let window = self.imp();

        // IMPLEMENTATION_PLAN.md / ADR-0003: compositing the (software-on-Nvidia)
        // WebKit UI over the per-frame-changing video pins a CPU core. Caching a
        // texture of the UI is impossible (the WebView rasterizes empty) and a
        // subsurface is declined at fractional scale, so neither "separate the
        // layers" trick works. `FreezeOverlay` instead drops the UI's render
        // nodes from the scene while the player chrome is hidden (driven by
        // `set_ui_frozen`) — per-frame cost falls to the video-only floor at any
        // scale, on any driver, while the WebView stays mapped (keeps input and
        // focus). `graphics_offload` stays inside: a no-op at fractional scale,
        // harmless, and preserves the offload win at integer scale.
        // FreezeOverlay is the Nvidia-only fix for the software-WebKit
        // per-frame recomposite. It cuts playback CPU to the video floor
        // (validated ~90%→~21%) but currently leaves stale WebKit-subsurface
        // artifacts when it hides the UI on Nvidia/Wayland (see
        // EXPERIMENT-nvidia.md). Gated behind an env flag so the shipping
        // default stays artifact-free (the PR #108 baseline is already the big
        // win on AMD/Intel, where no overlay freezing is needed); opt in with
        // STREMIO_FREEZE_OVERLAY=1 for testing the Nvidia path.
        if std::env::var_os("STREMIO_FREEZE_OVERLAY").is_some() {
            let freeze = FreezeOverlay::new(widget);
            window.overlay.add_overlay(&freeze);
            window.ui_overlay.replace(Some(freeze));
        } else {
            window.overlay.add_overlay(widget);
        }
    }

    /// Exclude (`true`) or restore (`false`) the WebKit UI overlay in the render
    /// scene. Driven by the freeze controller: frozen while playback is active
    /// and the player chrome is hidden, restored on any input or UI activity.
    pub fn set_ui_frozen(&self, frozen: bool) {
        if let Some(overlay) = self.imp().ui_overlay.borrow().as_ref() {
            overlay.set_frozen(frozen);
        }
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.imp().show_header(!fullscreen);
        self.set_fullscreened(fullscreen);
    }

    pub fn connect_visibility<T: Fn(bool) + 'static>(&self, callback: T) {
        self.connect_visible_notify(move |window| {
            callback(window.is_visible());
        });
    }

    fn request_backgound(&self) {
        self.imp().request_backgound();
    }

    pub fn disable_idling(&self) {
        self.imp().disable_idling();
    }

    pub fn enable_idling(&self) {
        self.imp().enable_idling();
    }

    pub fn open_uri(&self, uri: String) {
        self.imp().open_uri(uri);
    }

    pub fn open_file(&self, file_path: String) {
        self.imp().open_file(file_path);
    }
}

fn graphics_offload(widget: &impl IsA<Widget>) -> gtk::GraphicsOffload {
    gtk::GraphicsOffload::builder()
        .vexpand(true)
        .hexpand(true)
        .child(widget)
        .build()
}
