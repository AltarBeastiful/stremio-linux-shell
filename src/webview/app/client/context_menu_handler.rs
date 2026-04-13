use std::os::raw::c_int;

use crate::cef_impl;

const COPY_MAGNET_LINK_COMMAND_ID: c_int = 26500;

cef_impl!(
    prefix = "WebView",
    name = ContextMenuHandler,
    sys_type = cef_dll_sys::cef_context_menu_handler_t,
    {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            if let Some(model) = model {
                model.clear();

                if let Some(params) = params {
                    let link_url = CefString::from(&params.link_url()).to_string();
                    if link_url.starts_with("magnet:") {
                        let label = CefString::from("Copy Magnet Link");
                        model.add_item(COPY_MAGNET_LINK_COMMAND_ID, Some(&label));
                    }
                }
            }
        }

        fn on_context_menu_command(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            params: Option<&mut ContextMenuParams>,
            command_id: c_int,
            _event_flags: EventFlags,
        ) -> c_int {
            if command_id == COPY_MAGNET_LINK_COMMAND_ID {
                if let Some(params) = params {
                    let link_url = CefString::from(&params.link_url()).to_string();
                    if !link_url.is_empty() {
                        gtk::glib::idle_add(move || {
                            use gtk::prelude::ClipboardExt;
                            let atom = gtk::gdk::Atom::intern("CLIPBOARD", false);
                            let clipboard = gtk::Clipboard::get(&atom);
                            clipboard.set_text(&link_url);
                            gtk::glib::ControlFlow::Break
                        });
                    }
                }
                return 1;
            }
            0
        }
    }
);
