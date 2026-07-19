use std::cell::{Cell, RefCell};

use gtk::{glib, prelude::*, subclass::prelude::*};

/// See [`super::FreezeOverlay`] for the rationale (ADR-0003 / IMPLEMENTATION_PLAN.md).
#[derive(Default)]
pub struct FreezeOverlay {
    pub(super) child: RefCell<Option<gtk::Widget>>,
    /// While set, `snapshot()` contributes no render nodes for the child.
    pub(super) frozen: Cell<bool>,
}

#[glib::object_subclass]
impl ObjectSubclass for FreezeOverlay {
    const NAME: &'static str = "StremioFreezeOverlay";
    type Type = super::FreezeOverlay;
    type ParentType = gtk::Widget;
}

impl ObjectImpl for FreezeOverlay {
    fn dispose(&self) {
        if let Some(child) = self.child.borrow_mut().take() {
            child.unparent();
        }
    }
}

impl WidgetImpl for FreezeOverlay {
    fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
        self.child
            .borrow()
            .as_ref()
            .map(|child| child.measure(orientation, for_size))
            .unwrap_or((0, 0, -1, -1))
    }

    fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
        if let Some(child) = self.child.borrow().as_ref() {
            child.allocate(width, height, baseline, None);
        }
    }

    fn snapshot(&self, snapshot: &gtk::Snapshot) {
        // Frozen: contribute zero render nodes. Because the video underlay
        // changes every frame, GSK would otherwise re-composite this (software,
        // on Nvidia) overlay 60×/s. Skipping the child's snapshot entirely drops
        // the per-frame cost to the video-only floor at any scale, on any driver.
        // The child stays parented and mapped, so it keeps input, focus, and its
        // last-rendered frame — unfreezing is an instant `queue_draw`.
        if self.frozen.get() {
            return;
        }
        if let Some(child) = self.child.borrow().as_ref() {
            self.obj().snapshot_child(child, snapshot);
        }
    }
}
