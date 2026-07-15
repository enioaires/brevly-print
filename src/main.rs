//! Brevly Print — entry point.
//!
//! Startup order (D-17):
//!   1. Velopack bootstrapper (Windows-only, must be the very first call — OQ3)
//!   2. `init_app_dir()` — creates `BrevlyPrint/` in the platform data dir
//!   3. `open_and_migrate()` — migrates `state.db` to schema v1
//!   4. `config_store::set/get` — one round-trip to prove the store is live
//!   5. `credential_store()` + `save/load` — one round-trip through the `CredentialStore` trait
//!   6. `EventLoop::<UserEvent>::with_user_event()` + `run_app()` — drives the egui window

use anyhow::Context as _;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
};

use brevly_print::{
    app_dir::init_app_dir,
    config_store,
    credential_store::credential_store,
};

// ── UserEvent placeholder (tray wiring is Phase 3) ──────────────────────────

/// Events sent from background tasks / OS integrations into the winit event loop.
///
/// Phase 3 will add `TrayIconEvent` and `MenuEvent` variants here.
#[derive(Debug)]
enum UserEvent {
    // Phase 3: TrayIconEvent(tray_icon::TrayIconEvent),
    // Phase 3: MenuEvent(tray_icon::menu::MenuEvent),
}

// ── Application state ────────────────────────────────────────────────────────

/// The top-level `ApplicationHandler` that drives the winit event loop.
struct App {
    /// The spike window renderer; `None` until `resumed()` creates the window and wgpu context.
    window: Option<brevly_print::spike_window::SpikeWindow>,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // Create window + wgpu surface + egui renderer here (Pitfall 2: must be in resumed()).
        // Skip if already initialised (resumed() may be called more than once on some platforms).
        if self.window.is_none() {
            match brevly_print::spike_window::SpikeWindow::new(event_loop) {
                Ok(w) => self.window = Some(w),
                Err(e) => {
                    eprintln!("[brevly-print] Failed to create window: {e:#}");
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(spike) = self.window.as_mut() else { return };

        // Forward to egui-winit first; if the event was consumed by egui, don't process it further.
        let response = spike.handle_input(&event);
        if response.consumed {
            // Still need to redraw if egui requested it.
            if response.repaint {
                spike.window().request_redraw();
            }
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                if let Err(e) = spike.draw() {
                    eprintln!("[brevly-print] Draw error: {e:#}");
                }
            }
            WindowEvent::CloseRequested => {
                println!("[brevly-print] Window closed — exiting.");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                spike.resize(size);
                spike.window().request_redraw();
            }
            _ => {
                // Request a redraw so egui stays responsive (e.g., cursor move, focus change).
                spike.window().request_redraw();
            }
        }
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserEvent,
    ) {
        match event {
            // Phase 3: handle tray / menu events here.
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Request a redraw every time the event loop is idle so egui can process
        // animations and pending repaints.
        if let Some(spike) = self.window.as_ref() {
            spike.window().request_redraw();
        }
    }
}

// ── main ─────────────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    // OQ3: Velopack bootstrapper MUST be the very first call in main() (Windows-only).
    // On Linux this is a no-op compile-time guard.
    #[cfg(windows)]
    velopack::VelopackApp::build().run();
    // (Non-Windows: no velopack bootstrapper call needed — the update flow is Windows-only.)

    // ── Startup wiring (D-17: init_app_dir before any file ops) ─────────────
    let app_dir = init_app_dir().context("Failed to create BrevlyPrint app directory")?;
    println!("[brevly-print] App dir: {}", app_dir.display());

    // Migrate state.db to schema v1 (D-12, D-13, D-14).
    let db_path = app_dir.join("state.db");
    let conn = config_store::open_and_migrate(&db_path)
        .context("Failed to open or migrate state.db")?;
    println!("[brevly-print] state.db migrated (user_version=1)");

    // Probe config store: write + read back one row.
    config_store::set(&conn, "skeleton_probe", "ok")
        .context("Failed to write skeleton_probe to config")?;
    let probe_val = config_store::get(&conn, "skeleton_probe")
        .context("Failed to read skeleton_probe from config")?;
    println!("[brevly-print] config skeleton_probe = {:?}", probe_val);
    assert_eq!(probe_val.as_deref(), Some("ok"), "config round-trip failed");

    // Probe credential store: save + load through the CredentialStore trait (T-1-01).
    let cred = credential_store(&app_dir);
    use brevly_print::credential_store::CredentialStore as _;
    cred.save(b"skeleton-dummy")
        .context("Credential save failed")?;
    let loaded = cred.load().context("Credential load failed")?;
    assert_eq!(loaded, b"skeleton-dummy", "credential round-trip mismatch");
    println!("[brevly-print] Credential round-trip: OK");

    // ── winit event loop ─────────────────────────────────────────────────────
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("Failed to build winit event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);

    // Phase 3: wire tray/menu event forwarding via event_loop.create_proxy() here.

    let mut app = App { window: None };
    event_loop.run_app(&mut app).context("Event loop error")?;

    Ok(())
}
