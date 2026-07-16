//! Brevly Print library crate.
//!
//! Exposes the portable core: app-dir initializer, config store,
//! credential store (trait + cfg-gated platform impls), Noren HTTP client,
//! machine-ID reader, Printer trait + Linux stub, and the activation window.

pub mod activation_state;
pub mod activation_window;
pub mod app_dir;
pub mod config_store;
pub mod credential_store;
pub mod health_state;
pub mod machine_id;
pub mod noren_client;
pub mod printer;
// spike_window kept for reference but superseded by activation_window in Phase 2.
// Removed from main.rs startup flow.

// tray_runtime is Windows-only (cfg gate is inside the file itself via #![cfg(windows)],
// but the module declaration in lib.rs does not need a cfg gate because the file-level
// #![cfg(windows)] prevents the module body from compiling on non-Windows):
pub mod tray_runtime;

pub use app_dir::init_app_dir;
