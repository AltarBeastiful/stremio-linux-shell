mod imp;

#[cfg(test)]
mod tests;

use gtk::{glib, prelude::*, subclass::prelude::*};

glib::wrapper! {
    /// Composites a **cached texture** of its child instead of re-processing the
    /// child every frame.
    ///
    /// The Stremio UI is a transparent WebKitGTK overlay over the video. On the
    /// proprietary Nvidia driver WebKit renders it in software (a `GskCairoNode`);
    /// because it overlaps the video and the video changes every frame, GSK
    /// re-processes that software surface on the CPU 60×/s even though the UI is
    /// unchanged — pinning a core (DEVLOG §11, ADR-0003).
    ///
    /// This widget breaks that coupling *without* a Wayland subsurface (which GTK
    /// declines at fractional scale). It keeps the child parented (so it renders
    /// and receives input) but draws an immutable, texture-backed snapshot of it,
    /// refreshed only when the child's own contents change. Between UI changes,
    /// `snapshot()` is not re-run, so GSK reuses a cached texture node and never
    /// touches the (software) child — a GPU blend per frame at any scale, on any
    /// driver.
    pub struct CachedOverlay(ObjectSubclass<imp::CachedOverlay>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CachedOverlay {
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

        // The paintable both renders the child (on demand, via `current_image`)
        // and tells us when its contents change so we can restale the cache.
        let paintable = gtk::WidgetPaintable::new(Some(&child));
        paintable.connect_invalidate_contents(glib::clone!(
            #[weak(rename_to = this)]
            self,
            move |_| this.invalidate()
        ));

        *imp.paintable.borrow_mut() = Some(paintable);
        *imp.child.borrow_mut() = Some(child);
        imp.dirty.set(true);
        self.queue_resize();
    }

    pub fn child(&self) -> Option<gtk::Widget> {
        self.imp().child.borrow().clone()
    }

    /// Restale the cached snapshot so the next draw refreshes it. Driven by the
    /// child's `invalidate-contents` — i.e. real UI changes, not video frames.
    pub(crate) fn invalidate(&self) {
        self.imp().dirty.set(true);
        self.queue_draw();
    }

    #[cfg(test)]
    pub(crate) fn is_dirty(&self) -> bool {
        self.imp().dirty.get()
    }
}
