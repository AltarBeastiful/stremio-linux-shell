use super::CachedOverlay;
use gtk::prelude::*;

/// GTK must be initialised (needs a display) before widgets can be built. On a
/// headless box `gtk::init` fails; skip rather than fail the suite there.
fn gtk_ready() -> bool {
    gtk::init().is_ok()
}

#[test]
fn new_parents_the_child_and_exposes_it() {
    if !gtk_ready() {
        return;
    }
    let label = gtk::Label::new(Some("hi"));
    let overlay = CachedOverlay::new(&label);

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
}

#[test]
fn set_child_reparents_the_previous_one() {
    if !gtk_ready() {
        return;
    }
    let first = gtk::Label::new(Some("a"));
    let overlay = CachedOverlay::new(&first);

    let second = gtk::Label::new(Some("b"));
    overlay.set_child(&second);

    assert!(first.parent().is_none(), "the old child is unparented");
    assert_eq!(
        second.parent().as_ref(),
        Some(overlay.upcast_ref::<gtk::Widget>())
    );
    assert_eq!(
        overlay.child().as_ref(),
        Some(second.upcast_ref::<gtk::Widget>())
    );
}

#[test]
fn starts_dirty_so_it_renders_once_and_reinvalidates_on_change() {
    if !gtk_ready() {
        return;
    }
    let overlay = CachedOverlay::new(&gtk::Label::new(Some("x")));
    assert!(
        overlay.is_dirty(),
        "a fresh overlay must take a first snapshot"
    );

    // `invalidate` is exactly what the child's `invalidate-contents` callback
    // calls; a UI change must restale the cache.
    overlay.invalidate();
    assert!(overlay.is_dirty());
}

#[test]
fn measures_to_the_child() {
    if !gtk_ready() {
        return;
    }
    let label = gtk::Label::new(Some("measured"));
    let overlay = CachedOverlay::new(&label);

    let (_, overlay_nat, _, _) =
        WidgetExt::measure(&overlay, gtk::Orientation::Horizontal, -1);
    let (_, label_nat, _, _) =
        WidgetExt::measure(&label, gtk::Orientation::Horizontal, -1);
    assert_eq!(
        overlay_nat, label_nat,
        "the overlay reports the child's size so layout is unchanged"
    );
}
