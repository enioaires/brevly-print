//! Brevly Print library crate.
//!
//! Exposes the portable core: app-dir initializer, config store,
//! credential store (trait + cfg-gated platform impls), Noren HTTP client,
//! machine-ID reader, and Printer trait + Linux stub.

pub mod app_dir;
pub mod config_store;
pub mod credential_store;
pub mod machine_id;
pub mod noren_client;
pub mod printer;
pub mod spike_window;

pub use app_dir::init_app_dir;
