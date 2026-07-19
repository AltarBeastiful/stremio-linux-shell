use super::FreezeOverlay;
use gtk::prelude::*;

/// GTK needs a display and may only be initialised on one thread per process.
/// libtest runs each `#[test]` on its own thread, so a multi-test GTK suite
/// panics ("initialize GTK from two different threads") on every test after the
/// first. We therefore keep the whole widget contract in ONE test (single
/// thread, single init) and guard it: init if we can, skip if GTK was already
/// brought up on another thread or there is no display (headless CI).
fn gtk_ready() -> bool {
    if gtk::is_initialized() {
        return gtk::is_initialized_main_thread();
    }
    gtk::init().is_ok()
}

#[test]
fn freeze_overlay_widget_contract() {
    if !gtk_ready() {
        return;
    }

    // new() parents the child and exposes it (so it renders and gets input).
    let label = gtk::Label::new(Some("hi"));
    let overlay = FreezeOverlay::new(&label);
    assert_eq!(
        overlay.child().as_ref(),
        Some(label.upcast_ref::<gtk::Widget>()),
        "child() returns the widget it was built with"
    );
    assert_eq!(
        label.parent().as_ref(),
        Some(overlay.upcast_ref::<gtk::Widget>()),
        "the child is parented to the overlay so it renders and gets input"
    );

    // A fresh overlay is visible (not frozen): no UX change until we freeze.
    assert!(
        !overlay.is_frozen(),
        "a fresh overlay is visible (not frozen)"
    );

    // measure() delegates to the child so layout is unchanged.
    let (_, overlay_nat, _, _) = WidgetExt::measure(&overlay, gtk::Orientation::Horizontal, -1);
    let (_, label_nat, _, _) = WidgetExt::measure(&label, gtk::Orientation::Horizontal, -1);
    assert_eq!(
        overlay_nat, label_nat,
        "the overlay reports the child's size so layout is unchanged"
    );

    // set_frozen toggles the render-scene exclusion state.
    overlay.set_frozen(true);
    assert!(overlay.is_frozen(), "freezing excludes the child from the scene");
    overlay.set_frozen(false);
    assert!(!overlay.is_frozen(), "unfreezing restores the child");

    // set_child reparents the previous child.
    let second = gtk::Label::new(Some("b"));
    overlay.set_child(&second);
    assert!(label.parent().is_none(), "the old child is unparented");
    assert_eq!(
        second.parent().as_ref(),
        Some(overlay.upcast_ref::<gtk::Widget>())
    );
    assert_eq!(
        overlay.child().as_ref(),
        Some(second.upcast_ref::<gtk::Widget>())
    );
}

// The actual node-suppression (frozen => zero render nodes => video-only CPU
// floor) needs a realised/mapped surface to observe and is proven by
// `examples/overlay_bench.rs` (BENCH_MODE=frozen ≈ the `none` row) and the live
// GTK-Inspector check, not a headless unit test — see IMPLEMENTATION_PLAN.md.
