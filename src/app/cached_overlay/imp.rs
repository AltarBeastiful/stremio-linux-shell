use std::cell::{Cell, RefCell};

use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

/// See [`super::CachedOverlay`] for the rationale (ADR-0003).
#[derive(Default)]
pub struct CachedOverlay {
    pub(super) child: RefCell<Option<gtk::Widget>>,
    pub(super) paintable: RefCell<Option<gtk::WidgetPaintable>>,
    /// Immutable, texture-backed snapshot of the child, refreshed only when the
    /// child actually changes — never per video frame.
    pub(super) cached: RefCell<Option<gdk::Paintable>>,
    pub(super) dirty: Cell<bool>,
    /// Last size we captured at. A resize or scale change re-lays-out the child
    /// at a new size; recapture then rather than stretch the old texture.
    pub(super) last_size: Cell<(i32, i32)>,
}

#[glib::object_subclass]
impl ObjectSubclass for CachedOverlay {
    const NAME: &'static str = "StremioCachedOverlay";
    type Type = super::CachedOverlay;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for CachedOverlay {
    fn dispose(&self) {
        if let Some(child) = self.child.borrow_mut().take() {
            child.unparent();
        }
    }
}

impl WidgetImpl for CachedOverlay {
    fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
        self.child
            .borrow()
            .as_ref()
            .map(|child| child.measure(orientation, for_size))
            .unwrap_or((0, 0, -1, -1))
    }

    fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
        // A size change makes the cached texture the wrong size; restale it so
        // the next snapshot re-captures at the new size instead of scaling.
        if self.last_size.replace((width, height)) != (width, height) {
            self.dirty.set(true);
        }
        if let Some(child) = self.child.borrow().as_ref() {
            child.allocate(width, height, baseline, None);
        }
    }

    fn snapshot(&self, snapshot: &gtk::Snapshot) {
        let obj = self.obj();
        let (width, height) = (obj.width(), obj.height());
        if width <= 0 || height <= 0 {
            return;
        }

        // Refresh the cached texture only when the child's contents changed
        // (`WidgetPaintable::invalidate-contents`, wired in `set_child`) — i.e.
        // when the UI actually updates, not on every video frame. `snapshot()`
        // itself only runs when *this* widget is invalidated (`queue_draw`),
        // which we do from that same signal, so between UI changes GSK reuses
        // this widget's cached render node and never touches the child.
        let stale = self.dirty.get() || self.cached.borrow().is_none();
        if stale {
            if let Some(paintable) = self.paintable.borrow().as_ref() {
                *self.cached.borrow_mut() = Some(paintable.current_image());
                self.dirty.set(false);
            }
        }

        if let Some(image) = self.cached.borrow().as_ref() {
            image.snapshot(snapshot.upcast_ref::<gdk::Snapshot>(), width as f64, height as f64);
        }
    }
}
