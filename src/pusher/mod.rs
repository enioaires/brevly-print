//! Pusher Channels WebSocket client — reconnect loop, channel auth, event dispatch.
//!
//! Cross-platform: no `#[cfg(windows)]` guard needed. The Pusher module itself
//! is portable (tokio-tungstenite compiles on Linux). The spawn site in
//! `main.rs` Runtime mode is already in a Windows-only context.

pub mod backoff;
pub mod client;
pub mod protocol;

pub use protocol::{PrintEvent, PusherConfig};
