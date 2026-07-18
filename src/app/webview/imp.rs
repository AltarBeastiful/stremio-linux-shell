use adw::subclass::prelude::*;
use gtk::{
    GestureClick,
    gdk::RGBA,
    glib::{self, clone},
    prelude::*,
};
use webkit::{WebView as WebKitWebView, prelude::*};

#[derive(Default)]
pub struct WebView {
    pub webview: WebKitWebView,
}

#[glib::object_subclass]
impl ObjectSubclass for WebView {
    const NAME: &'static str = "WebView";
    type Type = super::WebView;
    type ParentType = gtk::Box;
}

impl ObjectImpl for WebView {
    fn constructed(&self) {
        self.parent_constructed();

        let object = self.obj();

        self.webview.set_vexpand(true);
        self.webview.set_hexpand(true);
        self.webview
            .set_background_color(&RGBA::new(0.0, 0.0, 0.0, 0.0));

        if let Some(settings) = WebViewExt::settings(&self.webview) {
            settings.set_enable_media(false);
            settings.set_enable_media_capabilities(false);
            settings.set_enable_media_stream(false);
            settings.set_enable_webaudio(false);

            // WebKit's hardware-acceleration policy defaults to `Never` here
            // (webkit6 exposes only Always/Never), i.e. no accelerated
            // compositing — the UI renders in software. Force `Always` so WebKit
            // uses the GPU where it can. Necessary but not sufficient on Nvidia
            // (the transparent overlay still snapshots to a GskCairoNode), which
            // is why the per-frame recomposite is also addressed by CachedOverlay
            // (ADR-0003).
            settings.set_hardware_acceleration_policy(webkit::HardwareAccelerationPolicy::Always);
        }

        let gesture = GestureClick::new();
        gesture.set_button(0);
        gesture.connect_pressed(clone!(
            #[weak(rename_to = webview)]
            self.webview,
            move |gesture, _, _, _| {
                let button = gesture.current_button();
                match button {
                    8 => webview.go_back(),
                    9 => webview.go_forward(),
                    _ => {}
                }
            }
        ));
        self.webview.add_controller(gesture);

        object.append(&self.webview);
    }
}

impl WidgetImpl for WebView {}
impl BoxImpl for WebView {}
