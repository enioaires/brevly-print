//! Smoke test: wgpu adapter availability.
//!
//! This test is `#[ignore]`d by default so headless CI (no GPU / no software Vulkan)
//! stays green. It is run **manually** or in interactive CI where a GPU or software
//! rasterizer (lavapipe on Linux, WARP DX12 on Windows) is available.
//!
//! ## How to run
//!
//! ```bash
//! cargo test -- --ignored              # run ALL ignored tests
//! cargo test test_wgpu_adapter_available -- --ignored   # run this test specifically
//! ```
//!
//! On Linux (headless CI, no lavapipe): the test is skipped by default.
//! On Linux (interactive with Vulkan/lavapipe): set `WGPU_BACKEND=vulkan` if needed.
//! On Windows CI (WARP): set `WGPU_BACKEND=dx12` in the CI environment.
//!
//! ## Why `#[ignore]`
//!
//! See RESEARCH.md Pitfall 3 + cross-platform addendum landmine:
//! `ubuntu-latest` GHA runners have no GPU and no software Vulkan by default.
//! `wgpu::Adapter::request_adapter()` returns `None` without a GPU or software rasterizer.
//! Running this test in headless CI would cause a false panic ("adapter not found").
//!
//! The non-render tests (SQLite schema, config store, app-dir init, credential trait)
//! have no GPU dependency and always run in CI — only this render-path test is gated.

/// Verify that wgpu can find a compatible adapter on this machine.
///
/// This is the minimal proof that the wgpu backend is functional:
/// if an adapter is returned, the full egui-wgpu rendering pipeline can initialize.
///
/// **Marked `#[ignore]`** — run manually with `cargo test -- --ignored`.
#[test]
#[ignore = "requires GPU or software rasterizer (lavapipe/WARP); not available in headless CI"]
fn test_wgpu_adapter_available() {
    let instance = wgpu::Instance::default();

    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("Failed to create tokio runtime");

    let adapter = rt.block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None, // no surface needed for adapter enumeration
        force_fallback_adapter: false,
    }));

    // wgpu 29: request_adapter returns Result<Adapter, RequestAdapterError>
    assert!(
        adapter.is_ok(),
        "No wgpu adapter found. \
         On Linux: install lavapipe (mesa-vulkan-drivers) or set WGPU_BACKEND=vulkan. \
         On Windows: set WGPU_BACKEND=dx12 to use WARP software rasterizer. \
         Error: {:?}",
        adapter.err()
    );

    let adapter = adapter.unwrap();
    let info = adapter.get_info();
    println!(
        "wgpu adapter found: {:?} (backend={:?}, device_type={:?})",
        info.name, info.backend, info.device_type
    );
}
