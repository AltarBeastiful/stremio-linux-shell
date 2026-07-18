//! Micro-benchmark for the video-overlay compositing cost (DEVLOG §11/§16,
//! ADR-0003). It reproduces the shape of the bug *without* WebKit or playback:
//! a `GtkGLArea` that re-renders every frame (standing in for the video) with a
//! software UI drawn on top, then measures this process's own CPU time over a
//! fixed wall-clock window. Because the only moving part is the compositor, the
//! CPU delta between modes is the per-frame overlay-compositing cost.
//!
//! Modes (`BENCH_MODE`):
//!   none    — GLArea only, no overlay (floor: cost of the churning underlay)
//!   cairo   — a `GtkDrawingArea` overlay → its snapshot is a `GskCairoNode`,
//!             the exact node WebKit produces in software on Nvidia
//!   cached  — that same DrawingArea wrapped in the real `CachedOverlay`
//!   texture — the same pixels pre-rasterised to a `GdkTexture`, shown via
//!             `GtkPicture` (the ideal: a guaranteed GPU-blit each frame)
//!
//! `cairo` vs `texture` isolates whether GSK re-rasterises an unchanged cairo
//! node every frame (the hypothesised root cause); `cached` says whether the
//! current CachedOverlay actually reaches the `texture` floor or stays at
//! `cairo`. Env: BENCH_SECS (default 10), BENCH_SIZE (default 1280x720).
//!
//! Run: `cargo run --release --example overlay_bench` with BENCH_MODE set.

#[path = "../src/app/cached_overlay/mod.rs"]
mod cached_overlay;

use std::{
    cell::Cell,
    env,
    rc::Rc,
    time::Instant,
};

use cached_overlay::CachedOverlay;
use gtk::{gdk, glib, prelude::*};

/// Read this process's consumed CPU seconds (user+system) from /proc/self/stat.
fn cpu_seconds() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // The comm field is parenthesised and may contain spaces/`)`, so split on
    // the last `)` and index the remaining whitespace-separated fields.
    let after = stat.rsplit_once(')').map(|(_, r)| r).unwrap_or("");
    let f: Vec<&str> = after.split_whitespace().collect();
    // After `)`: [0]=state … [11]=utime [12]=stime, in clock ticks.
    let utime: u64 = f.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = f.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);
    let hz = 100.0; // _SC_CLK_TCK is 100 on Linux/x86_64.
    (utime + stime) as f64 / hz
}

/// A moderately busy scene, drawn with cairo — a stand-in for the Stremio web
/// UI: an opaque-ish card, gradients, and many strokes so rasterisation is not
/// free. Deterministic, so `cairo` and `texture` composite identical pixels.
fn draw_ui(cr: &gtk::cairo::Context, w: f64, h: f64) {
    cr.set_source_rgba(0.06, 0.07, 0.09, 0.85);
    cr.rectangle(0.0, 0.0, w, w.min(h) * 0.16);
    let _ = cr.fill();

    for i in 0..40 {
        let t = i as f64 / 40.0;
        cr.set_source_rgba(0.20 + 0.6 * t, 0.30, 0.9 - 0.5 * t, 0.9);
        cr.rectangle(24.0 + t * (w - 260.0), 90.0, 180.0, 260.0);
        let _ = cr.fill();
        cr.set_line_width(2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.15);
        cr.rectangle(24.0 + t * (w - 260.0), 90.0, 180.0, 260.0);
        let _ = cr.stroke();
    }

    for i in 0..600 {
        let x = (i * 37 % w as i32) as f64;
        let y = h - 80.0 + (i % 7) as f64 * 6.0;
        cr.set_source_rgba(0.8, 0.85, 0.95, 0.5);
        cr.rectangle(x, y, 18.0, 3.0);
        let _ = cr.fill();
    }
}

fn build_drawing_area(w: i32, h: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(w);
    area.set_content_height(h);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_draw_func(|_, cr, w, h| draw_ui(cr, w as f64, h as f64));
    // BENCH_CHILD_DIRTY=1 makes the overlay invalidate itself every frame — the
    // pessimistic model of WebKit repainting per frame during playback. It
    // forces CachedOverlay to re-capture each frame, so `cached` under this knob
    // is the worst case that would justify approach (b) (freeze the idle
    // overlay) instead of (a) (cache it).
    if env::var("BENCH_CHILD_DIRTY").is_ok() {
        area.add_tick_callback(|a, _| {
            a.queue_draw();
            glib::ControlFlow::Continue
        });
    }
    area
}

/// Pre-rasterise `draw_ui` into a GdkTexture (via a cairo image surface).
fn build_texture(w: i32, h: i32) -> gdk::Texture {
    let surface =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, w, h).expect("surface");
    {
        let cr = gtk::cairo::Context::new(&surface).expect("cairo ctx");
        draw_ui(&cr, w as f64, h as f64);
    }
    surface.flush();
    let stride = surface.stride() as usize;
    let data = surface.take_data().expect("surface data");
    let bytes = glib::Bytes::from(&data[..]);
    gdk::MemoryTexture::new(
        w,
        h,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    )
    .upcast()
}

fn main() {
    let mode = env::var("BENCH_MODE").unwrap_or_else(|_| "cairo".into());
    let secs: u64 = env::var("BENCH_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let (w, h) = env::var("BENCH_SIZE")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1280, 720));

    let app = gtk::Application::builder()
        .application_id("com.stremio.OverlayBench")
        .build();

    let mode_c = mode.clone();
    app.connect_activate(move |app| {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(w)
            .default_height(h)
            .title(format!("overlay_bench [{mode_c}]"))
            .build();

        let overlay = gtk::Overlay::new();

        // Underlay: a GLArea that re-renders every frame — the "video".
        let gl = gtk::GLArea::new();
        gl.set_hexpand(true);
        gl.set_vexpand(true);
        gl.set_auto_render(true);
        let frames = Rc::new(Cell::new(0u64));
        {
            let frames = frames.clone();
            gl.connect_render(move |_, _| {
                // No drawing needed: the GLArea still swaps buffers every tick,
                // which is a real frame boundary and forces the overlay to be
                // re-composited — exactly the churn we want to measure.
                frames.set(frames.get() + 1);
                glib::Propagation::Stop
            });
        }
        // Drive continuous redraw.
        gl.add_tick_callback(|gl, _| {
            gl.queue_render();
            glib::ControlFlow::Continue
        });
        overlay.set_child(Some(&gl));

        // Optional: count how often the overlay child's contents actually
        // invalidate (WidgetPaintable::invalidate-contents) over the run — i.e.
        // how often a cache would have to refresh. Attached as a passive
        // observer so it does not perturb what it measures.
        let invalidations = Rc::new(Cell::new(0u64));
        let observe = |area: &gtk::DrawingArea, counter: &Rc<Cell<u64>>| {
            let paintable = gtk::WidgetPaintable::new(Some(area));
            let counter = counter.clone();
            paintable.connect_invalidate_contents(move |_| {
                counter.set(counter.get() + 1);
            });
            // Keep the paintable alive for the window's lifetime.
            unsafe { area.set_data("bench-observer", paintable) };
        };

        match mode_c.as_str() {
            "none" => {}
            "cairo" => {
                let area = build_drawing_area(w, h);
                observe(&area, &invalidations);
                overlay.add_overlay(&area);
            }
            "cached" => {
                let area = build_drawing_area(w, h);
                observe(&area, &invalidations);
                overlay.add_overlay(&CachedOverlay::new(&area));
            }
            "texture" => {
                let pic = gtk::Picture::for_paintable(&build_texture(w, h));
                pic.set_hexpand(true);
                pic.set_vexpand(true);
                overlay.add_overlay(&pic);
            }
            other => {
                eprintln!("unknown BENCH_MODE={other}");
                app.quit();
                return;
            }
        }

        window.set_child(Some(&overlay));
        window.present();

        // Measure once rendering is warm.
        let frames_m = frames.clone();
        let app_m = app.clone();
        let mode_m = mode_c.clone();
        glib::timeout_add_seconds_local(1, move || {
            let start_cpu = cpu_seconds();
            let start_frames = frames_m.get();
            let start_t = Instant::now();
            let frames_e = frames_m.clone();
            let app_e = app_m.clone();
            let mode_e = mode_m.clone();
            let inval_e = invalidations.clone();
            let start_inval = invalidations.get();
            glib::timeout_add_seconds_local(secs as u32, move || {
                let wall = start_t.elapsed().as_secs_f64();
                let cpu = cpu_seconds() - start_cpu;
                let nframes = frames_e.get() - start_frames;
                let ninval = inval_e.get() - start_inval;
                println!(
                    "mode={:<8} size={}x{} wall={:.2}s frames={} fps={:.1} cpu={:.2}s cpu%={:.1} child_invals={}",
                    mode_e,
                    w,
                    h,
                    wall,
                    nframes,
                    nframes as f64 / wall,
                    cpu,
                    cpu / wall * 100.0,
                    ninval,
                );
                app_e.quit();
                glib::ControlFlow::Break
            });
            glib::ControlFlow::Break
        });
    });

    let empty: Vec<String> = vec![];
    app.run_with_args(&empty);
}
