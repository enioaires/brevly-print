//! Brevly Print library crate.
//!
//! Exposes the portable core: app-dir initializer, config store,
//! and credential store (trait + cfg-gated platform impls).

pub mod app_dir;
pub mod config_store;
pub mod credential_store;
pub mod spike_window;

pub use app_dir::init_app_dir;
