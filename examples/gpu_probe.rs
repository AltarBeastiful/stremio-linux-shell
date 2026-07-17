//! Prints the GSK renderer that src/gpu.rs forces for this machine's GPU, or
//! `gtk-default` when it leaves GTK's choice alone. Drives
//! packaging/test-gsk-renderer.sh. src/gpu.rs is std-only, so this compiles
//! standalone with `rustc --edition 2024 examples/gpu_probe.rs` as well as with
//! `cargo run --example gpu_probe`.
#[path = "../src/gpu.rs"]
mod gpu;

fn main() {
    match gpu::preferred_gsk_renderer() {
        Some(renderer) => println!("{renderer}"),
        None => println!("gtk-default"),
    }
}
