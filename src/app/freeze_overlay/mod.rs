mod imp;

#[cfg(test)]
mod tests;

use gtk::{glib, prelude::*, subclass::prelude::*};

glib::wrapper! {
    /// Excludes its child from the render scene while **frozen** — its
    /// `snapshot()` contributes zero render nodes, so GSK never re-processes the
    /// child even when content beneath it (the video) changes every frame.
    ///
    /// The Stremio UI is a transparent WebKitGTK overlay over the video. On the
    /// proprietary Nvidia driver WebKit renders it in software (a `GskCairoNode`);
    /// because it overlaps the per-frame-changing video, GSK re-composites that
    /// software surface on the CPU 60×/s even while the UI is unchanged — pinning
    /// a core (DEVLOG §11/§18, ADR-0003). Caching a texture of the UI is
    /// impossible (`WidgetPaintable::current_image()` rasterizes the WebView
    /// empty) and a Wayland subsurface is declined at fractional scale, so neither
    /// "separate the layers" trick works.
    ///
    /// `FreezeOverlay` instead removes the UI from the scene *while it is
    /// invisible anyway* — the controls auto-hide for most of playback and
    /// subtitles are mpv-rendered, so nothing visible is lost. Unlike a texture
    /// cache there is no snapshot and no invalidation-correctness problem. Unlike
    /// `set_visible(false)`, the child is **not** unmapped, so it keeps keyboard
    /// focus, receives input (GTK4 picking is geometry-based), and does not flip
    /// WebKit's Page Visibility — so unfreezing is instant and flicker-free.
    /// Scale- and driver-independent by construction.
    pub struct FreezeOverlay(ObjectSubclass<imp::FreezeOverlay>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FreezeOverlay {
    pub fn new(child: &impl IsA<gtk::Widget>) -> Self {
        let obj: Self = glib::Object::new();
        obj.set_child(child);
        obj
    }

    pub fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        let imp = self.imp();

        if let Some(old) = imp.child.borrow_mut().take() {
            old.unparent();
        }

        let child = child.clone().upcast::<gtk::Widget>();
        child.set_parent(self);

        *imp.child.borrow_mut() = Some(child);
        self.queue_resize();
    }

    pub fn child(&self) -> Option<gtk::Widget> {
        self.imp().child.borrow().clone()
    }

    /// Exclude (`true`) or include (`false`) the child from the scene.
    /// Idempotent; only acts when the state actually changes.
    ///
    /// Skipping the child's `snapshot()` is enough to stop GSK re-compositing it
    /// (the CPU win), but it is NOT enough to make it disappear: WebKitGTK
    /// presents the UI through a Wayland subsurface that keeps displaying its
    /// last committed frame until the widget is unmapped — leaving a stale UI
    /// overlay stuck over the video (seekbar/top-bar/popups that "never redraw
    /// out"; toggling fullscreen, which remaps, clears them). So we also hide the
    /// child while frozen: unmapping tears the subsurface down, and showing it
    /// again restores a live UI. The child stays *parented* (so it is not
    /// destroyed and re-created), and the freeze controller unfreezes eagerly on
    /// any input, so interaction re-shows the UI immediately.
    pub fn set_frozen(&self, frozen: bool) {
        if self.imp().frozen.replace(frozen) != frozen {
            // NOTE: E1 (`queue_resize` on the child to force a re-commit) was
            // tried and FAILED — it did not clear the stale subsurface and caused
            // intermittent black screens. Reverted. See EXPERIMENT-nvidia.md.
            if let Some(child) = self.imp().child.borrow().as_ref() {
                child.set_visible(!frozen);
            }
            self.queue_draw();
        }
    }

    pub fn is_frozen(&self) -> bool {
        self.imp().frozen.get()
    }
}
