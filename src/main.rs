//! Brevly Print — native Windows print agent for Noren.
//!
//! This is a minimal placeholder. The real event loop (winit + egui + tray-icon)
//! is added in plan 01-03. This stub exists so `cargo build` passes from day one.

fn main() {
    // Initialize the app directory before any file operations (pitfall m2 / D-17).
    match brevly_print::app_dir::init_app_dir() {
        Ok(path) => println!("BrevlyPrint app dir: {}", path.display()),
        Err(e) => eprintln!("Failed to initialize app dir: {e}"),
    }
}
