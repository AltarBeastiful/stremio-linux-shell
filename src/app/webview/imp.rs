use adw::subclass::prelude::*;
use gtk::{gdk::RGBA, glib, prelude::*};
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
        }

        // Suppress the native WebKitGTK context menu so that the React app's
        // own right-click popup (which contains the Preload option) can appear.
        self.webview.connect_context_menu(|_, _, _| true);

        object.append(&self.webview);
    }
}

impl WidgetImpl for WebView {}
impl BoxImpl for WebView {}
