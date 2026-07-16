//! Brevly Print — entry point.
//!
//! Startup order (D-17):
//!   1. Velopack bootstrapper (Windows-only, must be the very first call — OQ3)
//!   2. `init_app_dir()` — creates `BrevlyPrint/` in the platform data dir
//!   3. `open_and_migrate()` — migrates `state.db` to schema v1
//!   4. `config_store::set/get` — one round-trip to prove the store is live
//!   5. Credential probe (ACT-07): NotFound/Corrupt → activation window, Ok → Phase 3 runtime
//!   6. `tokio::runtime::Builder::new_multi_thread()` runtime built BEFORE the event loop (Pitfall 3)
//!   7. `EventLoop::<UserEvent>::with_user_event()` + `run_app()` — drives the egui window

use anyhow::Context as _;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ControlFlow, EventLoop},
};

use brevly_print::{
    activation_window::ActivationWindow,
    app_dir::init_app_dir,
    config_store,
    credential_store::{credential_store, CredentialError, CredentialStore as _},
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
    /// The activation window renderer; `None` until `resumed()` creates the window.
    window: Option<ActivationWindow>,
    /// Persistent multi-thread tokio runtime handle (Pattern 2 / Pitfall 3).
    rt: tokio::runtime::Handle,
    /// Shared reqwest HTTP client (Pitfall 6: create once, reuse).
    http: reqwest::Client,
    /// Whether the startup credential check found NotFound/Corrupt (shows re-activation banner).
    is_reactivation: bool,
    /// App directory path (needed by save flow).
    app_dir: std::path::PathBuf,
    /// SQLite connection (needed by save flow config_store::set calls).
    conn: rusqlite::Connection,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // Create window + wgpu surface + egui renderer here (Pitfall 2: must be in resumed()).
        // Skip if already initialised (resumed() may be called more than once on some platforms).
        if self.window.is_none() {
            match ActivationWindow::new(
                event_loop,
                self.rt.clone(),
                self.http.clone(),
                self.is_reactivation,
                self.app_dir.clone(),
            ) {
                Ok(w) => self.window = Some(w),
                Err(e) => {
                    eprintln!("[brevly-print] Failed to create activation window: {e:#}");
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
        let Some(window) = self.window.as_mut() else { return };

        // Forward to egui-winit first; if the event was consumed by egui, don't process it further.
        let response = window.handle_input(&event);
        if response.consumed {
            // Still need to redraw if egui requested it.
            if response.repaint {
                window.window().request_redraw();
            }
            return;
        }

        match event {
            WindowEvent::RedrawRequested => {
                if let Err(e) = window.draw(&self.conn) {
                    eprintln!("[brevly-print] Draw error: {e:#}");
                }
                // Check if the window requested exit (save flow completed).
                if window.should_exit() {
                    println!("[brevly-print] Activation save complete — exiting.");
                    event_loop.exit();
                }
            }
            WindowEvent::CloseRequested => {
                println!("[brevly-print] Window closed — exiting.");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                window.resize(size);
                window.window().request_redraw();
            }
            _ => {
                // Request a redraw so egui stays responsive (e.g., cursor move, focus change).
                window.window().request_redraw();
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
        // animations, pending repaints, and the oneshot result polling (Pattern 2).
        if let Some(window) = self.window.as_ref() {
            window.window().request_redraw();
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

    // ── Build the multi-thread tokio runtime BEFORE the event loop (Pitfall 3) ──
    // Keep the Runtime alive in this scope for the entire process lifetime.
    // The `Handle` is cloned into `App` and passed to the activation window.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to build tokio runtime")?;
    let rt_handle = rt.handle().clone();

    // Build a shared reqwest client once (Pitfall 6).
    let http = reqwest::Client::new();

    // ── Startup wiring (D-17: init_app_dir before any file ops) ─────────────
    let app_dir = init_app_dir().context("Failed to create BrevlyPrint app directory")?;
    println!("[brevly-print] App dir: {}", app_dir.display());

    // Migrate state.db to schema v1 (D-12, D-13, D-14).
    let db_path = app_dir.join("state.db");
    let conn = config_store::open_and_migrate(&db_path)
        .context("Failed to open or migrate state.db")?;
    println!("[brevly-print] state.db migrated (user_version=1)");

    // ── Credential check (ACT-07) ────────────────────────────────────────────
    // NotFound|Corrupt → activation window (first run or re-activation)
    // Ok(_)           → Phase 3 runtime (already activated)
    // Io(e)           → fatal I/O error, propagate
    let cred = credential_store(&app_dir);
    let needs_activation = match cred.load() {
        Ok(_token) => {
            // Already activated. Phase 3 will start the tray runtime here.
            // For Phase 2, we log and exit 0 (no tray yet).
            println!("[brevly-print] Credential found — agent already activated. (Phase 3: start tray runtime here)");
            false
        }
        Err(CredentialError::NotFound) => {
            println!("[brevly-print] No credential found — opening activation window.");
            true
        }
        Err(CredentialError::Corrupt(_)) => {
            println!("[brevly-print] Credential corrupt — opening re-activation window.");
            true
        }
        Err(e) => {
            return Err(anyhow::anyhow!(e)).context("Credential I/O error on startup");
        }
    };

    // ── Phase 3 stub: if already activated, exit cleanly ─────────────────────
    if !needs_activation {
        println!("[brevly-print] Runtime phase not yet implemented (Phase 3). Exiting.");
        return Ok(());
    }

    // ── winit event loop ─────────────────────────────────────────────────────
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("Failed to build winit event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);

    // Phase 3: wire tray/menu event forwarding via event_loop.create_proxy() here.

    let mut app = App {
        window: None,
        rt: rt_handle,
        http,
        is_reactivation: matches!(cred.load(), Err(CredentialError::Corrupt(_))),
        app_dir: app_dir.clone(),
        conn,
    };
    event_loop.run_app(&mut app).context("Event loop error")?;

    // Keep runtime alive until process fully exits.
    drop(rt);
    Ok(())
}
