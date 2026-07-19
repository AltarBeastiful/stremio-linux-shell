use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use adw::{prelude::*, subclass::prelude::*};
use gtk::glib::{self, Properties, clone};
use tracing::error;

use crate::{
    app::{
        config::{APP_ID, APP_NAME, URI_SCHEME},
        ipc::{
            self,
            event::{IpcEvent, IpcEventMpv},
        },
        mpris::Mpris,
        tray::Tray,
        video::Video,
        webview::WebView,
        window::Window,
    },
    spawn_local, utils,
};

const PRELOAD_SCRIPT: &str = include_str!("ipc/preload.js");

/// Debounce so a stale "hidden" report arriving just after local input can't
/// re-freeze the overlay the user is actively driving.
const FREEZE_INPUT_DEBOUNCE: Duration = Duration::from_millis(500);

/// State for the freeze controller (IMPLEMENTATION_PLAN.md): the overlay is
/// frozen only while playback is active AND the player chrome is hidden AND the
/// user hasn't just interacted. Any local input restores it eagerly.
struct FreezeState {
    playing: Cell<bool>,
    ui_visible: Cell<bool>,
    last_input: Cell<Instant>,
}

impl FreezeState {
    fn apply(&self, window: &Window) {
        let frozen = self.playing.get()
            && !self.ui_visible.get()
            && self.last_input.get().elapsed() >= FREEZE_INPUT_DEBOUNCE;
        tracing::debug!(
            target: "freeze",
            "apply frozen={frozen} (playing={} ui_visible={})",
            self.playing.get(),
            self.ui_visible.get()
        );
        window.set_ui_frozen(frozen);
    }

    /// Local input: the UI is (about to be) visible again — unfreeze eagerly.
    fn note_input(&self) {
        self.last_input.set(Instant::now());
        self.ui_visible.set(true);
    }
}

#[derive(Properties, Default)]
#[properties(wrapper_type = super::Application)]
pub struct Application {
    #[property(get, set)]
    dev_mode: Cell<bool>,
    #[property(get, set)]
    startup_url: RefCell<String>,
    #[property(get, set)]
    decorations: Cell<bool>,
    tray: RefCell<Option<Tray>>,
    mpris: RefCell<Option<Mpris>>,
    window: RefCell<Option<Window>>,
    webview: RefCell<Option<WebView>>,
    deeplink: RefCell<Option<String>>,
}

#[glib::object_subclass]
impl ObjectSubclass for Application {
    const NAME: &'static str = "Application";
    type Type = super::Application;
    type ParentType = adw::Application;
}

#[glib::derived_properties]
impl ObjectImpl for Application {}

impl ApplicationImpl for Application {
    fn startup(&self) {
        self.parent_startup();

        let app = self.obj();
        app.setup_actions();
        app.setup_accels();
        app.setup_css();
    }

    fn activate(&self) {
        self.parent_activate();

        let app = self.obj();

        if let Some(window) = app.active_window() {
            window.present();
            return;
        }

        let tray = Tray::default();
        let video = Video::default();
        let mpris = Mpris::default();

        let startup_url = self.startup_url.borrow();
        let dev_mode = self.dev_mode.get();

        let webview = WebView::default();
        webview.load_uri(&startup_url);
        webview.inject_script(PRELOAD_SCRIPT);
        webview.dev_mode(dev_mode);

        let window = Window::new(&app);
        window.set_property("decorations", self.decorations.get());
        window.set_underlay(&video);
        window.set_overlay(&webview);

        // --- Freeze controller (IMPLEMENTATION_PLAN.md / DEVLOG §18): exclude the
        // WebKit overlay from the render scene while playback is active and the
        // player chrome is hidden, so GSK stops re-compositing the (software, on
        // Nvidia) UI over the video every frame. Restored eagerly on any input;
        // signals combine the injected `shell_ui` observer, playback state, and
        // capture-phase input controllers. ---
        let freeze = Rc::new(FreezeState {
            playing: Cell::new(false),
            ui_visible: Cell::new(true),
            last_input: Cell::new(Instant::now()),
        });

        webview.connect_ui_visibility(clone!(
            #[strong]
            freeze,
            #[weak]
            window,
            move |visible| {
                tracing::debug!(target: "freeze", "shell_ui ui_visible={visible}");
                freeze.ui_visible.set(visible);
                freeze.apply(&window);
            }
        ));

        video.connect_playback_started(clone!(
            #[strong]
            freeze,
            #[weak]
            window,
            move || {
                tracing::debug!(target: "freeze", "playback started");
                freeze.playing.set(true);
                freeze.apply(&window);
            }
        ));

        video.connect_playback_ended(clone!(
            #[strong]
            freeze,
            #[weak]
            window,
            move |_| {
                freeze.playing.set(false);
                freeze.apply(&window);
            }
        ));

        // Eager local unfreeze on any input, captured before the WebView so the
        // UI is back the same frame as the input that wakes stremio-web's
        // controls (the WebView is never unmapped, so it still gets the event).
        let motion = gtk::EventControllerMotion::new();
        motion.set_propagation_phase(gtk::PropagationPhase::Capture);
        motion.connect_motion(clone!(
            #[strong]
            freeze,
            #[weak]
            window,
            move |_, _, _| {
                freeze.note_input();
                window.set_ui_frozen(false);
            }
        ));
        window.add_controller(motion);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        scroll.connect_scroll(clone!(
            #[strong]
            freeze,
            #[weak]
            window,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _, _| {
                freeze.note_input();
                window.set_ui_frozen(false);
                glib::Propagation::Proceed
            }
        ));
        window.add_controller(scroll);

        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        key.connect_key_pressed(clone!(
            #[strong]
            freeze,
            #[weak]
            window,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _, _, _| {
                freeze.note_input();
                window.set_ui_frozen(false);
                glib::Propagation::Proceed
            }
        ));
        window.add_controller(key);

        video.connect_playback_started(clone!(
            #[weak]
            window,
            move || {
                window.disable_idling();
            }
        ));

        video.connect_playback_ended(clone!(
            #[weak]
            window,
            #[weak]
            webview,
            move |reason| {
                window.enable_idling();

                let message = ipc::create_response(IpcEvent::Mpv(IpcEventMpv::Ended((
                    reason.to_string(),
                    None,
                ))));
                webview.send(&message);
            }
        ));

        video.connect_mpv_property_change(clone!(
            #[weak]
            webview,
            move |name, value| {
                let message = ipc::create_response(IpcEvent::Mpv(IpcEventMpv::Change((
                    name.to_string(),
                    value,
                ))));

                webview.send(&message);
            }
        ));

        let deeplink = self.deeplink.clone();
        webview.connect_ipc(clone!(
            #[weak]
            app,
            #[weak]
            window,
            #[weak]
            video,
            #[weak]
            mpris,
            move |webview: WebView, message: &str| {
                if let Ok(event) = ipc::parse_request(message) {
                    match event {
                        IpcEvent::Init => {
                            let message = ipc::create_response(IpcEvent::Init);
                            webview.send(&message);
                        }
                        IpcEvent::Ready => {
                            if let Some(ref uri) = *deeplink.borrow() {
                                let message =
                                    ipc::create_response(IpcEvent::OpenMedia(uri.to_string()));
                                webview.send(&message);
                            }
                        }
                        IpcEvent::Fullscreen(state) => {
                            window.set_fullscreen(state);

                            let message = ipc::create_response(IpcEvent::Fullscreen(state));
                            webview.send(&message);
                        }
                        IpcEvent::MediaStatus(status) => {
                            mpris.set_status(status);
                        }
                        IpcEvent::MediaMetadata((title, artist, artwork)) => {
                            mpris.set_metadata(title, artist, artwork);
                        }
                        IpcEvent::Quit => {
                            app.quit();
                        }
                        IpcEvent::Mpv(event) => match event {
                            IpcEventMpv::Observe(name) => video.observe_mpv_property(name),
                            IpcEventMpv::Command((name, args)) => {
                                video.send_mpv_command(name, args)
                            }
                            IpcEventMpv::Set((name, value)) => video.set_mpv_property(name, value),
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        ));

        webview.connect_fullscreen(clone!(
            #[weak]
            window,
            move |fullscreen: bool| {
                window.set_fullscreen(fullscreen);
            }
        ));

        webview.connect_open_external(clone!(
            #[weak]
            window,
            move |data| {
                if data.starts_with("application/octet-stream") {
                    spawn_local!(async move {
                        match utils::download_file("playlist.m3u8", data).await {
                            Ok(file_path) => window.open_file(file_path),
                            Err(e) => error!("Failed to download file: {e}"),
                        }
                    });
                } else {
                    window.open_uri(data);
                }
            }
        ));

        window.connect_visibility(clone!(
            #[weak]
            webview,
            #[weak]
            tray,
            move |state| {
                let message = ipc::create_response(IpcEvent::Visibility(state));
                webview.send(&message);

                tray.update(state);
            }
        ));
        tray.connect_show(clone!(
            #[weak]
            window,
            move || {
                window.set_visible(true);
            }
        ));

        tray.connect_hide(clone!(
            #[weak]
            window,
            move || {
                window.set_visible(false);
            }
        ));

        tray.connect_quit(clone!(
            #[weak]
            app,
            move || {
                app.quit();
            }
        ));

        mpris.connect_status(clone!(
            #[weak]
            webview,
            move |paused| {
                let message = ipc::create_response(IpcEvent::MediaStatus(paused));
                webview.send(&message);
            }
        ));

        mpris.connect_raise(clone!(
            #[weak]
            window,
            move || {
                window.activate();
            }
        ));

        mpris.start(APP_ID, APP_NAME);

        window.present();

        *self.tray.borrow_mut() = Some(tray);
        *self.mpris.borrow_mut() = Some(mpris);
        *self.window.borrow_mut() = Some(window);
        *self.webview.borrow_mut() = Some(webview);
    }

    fn open(&self, files: &[gtk::gio::File], hint: &str) {
        self.parent_open(files, hint);

        if let Some(file) = files.first() {
            let uri = file.uri().to_string();
            if uri.starts_with(URI_SCHEME) {
                let mut deeplink = self.deeplink.borrow_mut();
                *deeplink = Some(uri.clone());

                if let Some(ref webview) = *self.webview.borrow() {
                    let message = ipc::create_response(IpcEvent::OpenMedia(uri));
                    webview.send(&message);
                }
            }
        }

        self.activate();
    }

    fn shutdown(&self) {
        if let Some(window) = self.window.take() {
            window.destroy();
        }

        self.parent_shutdown();
    }
}

impl GtkApplicationImpl for Application {}
impl AdwApplicationImpl for Application {}
