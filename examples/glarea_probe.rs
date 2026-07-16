//! CPU probe for the GSK renderer question.
//!
//! Reproduces `src/app/video/imp.rs`'s render path exactly — a GTK4 `GLArea`
//! plus a libmpv `RenderContext`, with the update callback driving
//! `queue_render` through a flume channel — but with no WebKit and no server,
//! so the only thing being measured is decode + GSK compositing.
//!
//! Measures its own CPU from /proc/self/stat over a steady-state window, after
//! a warmup, and prints hwdec-current plus achieved render fps.
//!
//!   cargo run --release --example glarea_probe -- <file> [warmup_s] [window_s]
//!
//! Vary the renderer across runs with GSK_RENDERER=vulkan|gl|opengl|cairo.

use gdk_wayland::{WaylandDisplay, wayland_client::Proxy};
use gtk::{
    gdk::GLContext,
    glib::{self, Propagation},
    prelude::*,
};
use libmpv2::{
    Mpv,
    render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType},
};
use std::{
    cell::{Cell, RefCell},
    env, fs,
    os::raw::c_void,
    ptr,
    rc::Rc,
    time::{Duration, Instant},
};

const CLK_TCK: f64 = 100.0;

fn get_proc_address(_context: &GLContext, name: &str) -> *mut c_void {
    epoxy::get_proc_addr(name) as _
}

/// Process-wide CPU time (all threads), in seconds.
fn cpu_seconds() -> f64 {
    let stat = fs::read_to_string("/proc/self/stat").expect("read /proc/self/stat");
    // Skip past comm, which may itself contain spaces/parens.
    let rest = &stat[stat.rfind(')').expect("malformed stat") + 2..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After comm and state, utime/stime are fields 14/15 of the original line.
    let utime: u64 = fields[11].parse().expect("utime");
    let stime: u64 = fields[12].parse().expect("stime");
    (utime + stime) as f64 / CLK_TCK
}

fn main() -> glib::ExitCode {
    let file = env::args()
        .nth(1)
        .expect("usage: glarea_probe <file> [warmup_s] [window_s]");
    let warmup: u64 = env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let window: u64 = env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let library =
        unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }.expect("load libepoxy");
    epoxy::load_with(|name| {
        unsafe { library.get::<_>(name.as_bytes()) }
            .map(|symbol| *symbol)
            .unwrap_or(ptr::null())
    });

    let app = gtk::Application::builder()
        .application_id("com.stremio.GlareaProbe")
        .build();

    app.connect_activate(move |app| {
        // Same initializer as Video::default(), minus the UI plumbing.
        let mpv = Mpv::with_initializer(|init| {
            init.set_property("vo", "libmpv")?;
            init.set_property("video-timing-offset", "0")?;
            init.set_property("video-sync", "audio")?;
            init.set_property("hwdec", "auto-safe")?;
            init.set_property("terminal", "yes")?;
            init.set_property("msg-level", "all=no")?;
            // No audio device: keeps the audio clock for video-sync=audio
            // without adding output cost or noise to the measurement.
            init.set_property("ao", "null")?;
            Ok(())
        })
        .expect("Failed to create mpv");

        let mpv = Rc::new(RefCell::new(mpv));
        let render_context: Rc<RefCell<Option<RenderContext>>> = Rc::new(RefCell::new(None));
        let frames = Rc::new(Cell::new(0u64));

        let area = gtk::GLArea::new();

        area.connect_realize({
            let mpv = mpv.clone();
            let render_context = render_context.clone();
            move |area| {
                area.make_current();
                if area.error().is_some() {
                    return;
                }
                let Some(context) = area.context() else {
                    return;
                };

                let mut mpv_ref = mpv.borrow_mut();
                let mpv_handle = unsafe { mpv_ref.ctx.as_mut() };

                let mut render_params = vec![
                    RenderParam::ApiType(RenderParamApiType::OpenGl),
                    RenderParam::InitParams(OpenGLInitParams {
                        get_proc_address,
                        ctx: context,
                    }),
                ];

                if let Ok(display) = area.display().downcast::<WaylandDisplay>()
                    && let Some(display) = display.wl_display()
                {
                    render_params.push(RenderParam::WaylandDisplay(
                        display.id().as_ptr() as *const c_void
                    ));
                }

                let mut ctx = RenderContext::new(mpv_handle, render_params)
                    .expect("Failed to create render context");

                let (sender, receiver) = flume::unbounded::<()>();
                glib::MainContext::default().spawn_local({
                    let area = area.clone();
                    async move {
                        while receiver.recv_async().await.is_ok() {
                            while receiver.try_recv().is_ok() {}
                            area.queue_render();
                        }
                    }
                });
                ctx.set_update_callback(move || {
                    sender.send(()).ok();
                });

                *render_context.borrow_mut() = Some(ctx);
            }
        });

        area.connect_render({
            let render_context = render_context.clone();
            let frames = frames.clone();
            move |area, _ctx| {
                let mut fbo = 0;
                unsafe {
                    epoxy::GetIntegerv(epoxy::FRAMEBUFFER_BINDING, &mut fbo);
                }
                let scale = area.scale_factor();
                if let Some(ref ctx) = *render_context.borrow() {
                    ctx.render::<GLContext>(fbo, area.width() * scale, area.height() * scale, true)
                        .expect("Failed to render");
                }
                frames.set(frames.get() + 1);
                Propagation::Stop
            }
        });

        let win = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(1280)
            .default_height(720)
            .title("glarea_probe")
            .child(&area)
            .build();
        win.present();

        mpv.borrow()
            .command("loadfile", &[&file])
            .expect("loadfile");

        // Baseline after warmup, report after the measurement window.
        glib::timeout_add_seconds_local(warmup as u32, {
            let mpv = mpv.clone();
            let frames = frames.clone();
            let app = app.clone();
            move || {
                let hwdec = mpv
                    .borrow()
                    .get_property::<String>("hwdec-current")
                    .unwrap_or_else(|_| "<unknown>".into());
                let base_cpu = cpu_seconds();
                let base_frames = frames.get();
                let base_at = Instant::now();

                glib::timeout_add_seconds_local(window as u32, {
                    let frames = frames.clone();
                    let app = app.clone();
                    let hwdec = hwdec.clone();
                    move || {
                        let elapsed = base_at.elapsed().as_secs_f64();
                        let cpu = cpu_seconds() - base_cpu;
                        let rendered = frames.get() - base_frames;
                        println!(
                            "RESULT renderer={} hwdec={} cpu={:.1}% fps={:.1} window={:.1}s",
                            env::var("GSK_RENDERER").unwrap_or_else(|_| "<default>".into()),
                            hwdec,
                            cpu / elapsed * 100.0,
                            rendered as f64 / elapsed,
                            elapsed
                        );
                        app.quit();
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });
    });

    // Don't let clap-less arg passing confuse GtkApplication.
    app.run_with_args::<&str>(&[])
}

// Silence unused-import warnings when Duration is only used indirectly.
#[allow(dead_code)]
fn _unused(_: Duration) {}
