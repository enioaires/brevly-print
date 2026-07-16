//! Brevly Print — entry point.
//!
//! Startup order (D-17):
//!   1. Velopack bootstrapper (Windows-only, must be the very first call — OQ3)
//!   2. Single-instance mutex guard (D-08/D-09) — silent exit if another instance running
//!   3. `init_app_dir()` — creates `BrevlyPrint/` in the platform data dir
//!   4. `open_and_migrate()` — migrates `state.db` to schema v1
//!   5. `config_store::set/get` — one round-trip to prove the store is live
//!   6. Credential probe (ACT-07): NotFound/Corrupt → activation window, Ok → Phase 3 runtime
//!   7. `tokio::runtime::Builder::new_multi_thread()` runtime built BEFORE the event loop (Pitfall 3)
//!   8. `EventLoop::<UserEvent>::with_user_event()` + `run_app()` — drives the egui window or tray

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
    health_state::HealthState,
    pusher::{run_pusher_loop, PrintEvent, PusherConfig},
    noren_client::noren_base_url,
};

#[cfg(windows)]
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError},
    System::Threading::CreateMutexW,
};
#[cfg(windows)]
use tray_icon::{TrayIconEvent};
#[cfg(windows)]
use tray_icon::menu::MenuEvent;
#[cfg(windows)]
use brevly_print::tray_runtime::{self, TrayRuntime};

// ── AppMode ──────────────────────────────────────────────────────────────────

/// Which startup path the application is in (no cfg gate — portable).
enum AppMode {
    /// First-run or re-activation: shows the egui activation window.
    Activation,
    /// Already-activated: runs invisibly as a tray agent.
    Runtime,
}

// ── UserEvent ────────────────────────────────────────────────────────────────

/// Events sent from background tasks / OS integrations into the winit event loop.
#[derive(Debug)]
enum UserEvent {
    #[cfg(windows)]
    TrayIconEvent(tray_icon::TrayIconEvent),
    #[cfg(windows)]
    MenuEvent(tray_icon::menu::MenuEvent),
    HealthChanged(HealthState),
}

// ── Application state ────────────────────────────────────────────────────────

/// The top-level `ApplicationHandler` that drives the winit event loop.
struct App {
    // === Phase 2 fields (keep) ===
    rt: tokio::runtime::Handle,
    http: reqwest::Client,
    app_dir: std::path::PathBuf,
    conn: rusqlite::Connection,

    // === Phase 3 additions ===
    /// AppMode: which startup path we are in.
    mode: AppMode,
    /// Current health state (Phase 3 seeds Connected; Phase 4 drives transitions).
    health: HealthState,
    /// Tray runtime (Windows-only, None in Activation mode and on Linux).
    #[cfg(windows)]
    tray_runtime: Option<TrayRuntime>,
    /// Activation window (Some only when Activation mode or on-demand Reativar).
    activation_window: Option<ActivationWindow>,
    /// is_reactivation flag for ActivationWindow constructor.
    is_reactivation: bool,

    // === Phase 4 additions ===
    /// Receiver for Phase 5 print worker handoff. Held here so the channel is not dropped
    /// (a dropped receiver would make every `tx.send` in the Pusher task fail). Phase 5
    /// will take this out of `Option` and own the receiver.
    _print_rx: Option<tokio::sync::mpsc::Receiver<PrintEvent>>,
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        #[cfg(windows)]
        if cause == winit::event::StartCause::Init {
            if matches!(self.mode, AppMode::Runtime) {
                // CRITICAL: tray creation must happen here, not before run_app().
                // See RESEARCH.md Pattern 1 — Win32 message pump must be running.
                match TrayRuntime::new(self.health) {
                    Ok(rt) => self.tray_runtime = Some(rt),
                    Err(e) => {
                        eprintln!("[brevly-print] Failed to create tray icon: {e:#}");
                        event_loop.exit();
                    }
                }
            }
        }
        // Suppress unused variable warning on Linux where the cfg block is empty.
        let _ = (event_loop, cause);
    }

    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        match self.mode {
            AppMode::Activation => {
                // Create window + wgpu surface + egui renderer here (Pitfall 2: must be in resumed()).
                // Skip if already initialised (resumed() may be called more than once on some platforms).
                if self.activation_window.is_none() {
                    match ActivationWindow::new(
                        event_loop,
                        self.rt.clone(),
                        self.http.clone(),
                        self.is_reactivation,
                        self.app_dir.clone(),
                    ) {
                        Ok(w) => self.activation_window = Some(w),
                        Err(e) => {
                            eprintln!("[brevly-print] Failed to create activation window: {e:#}");
                            event_loop.exit();
                        }
                    }
                }
            }
            AppMode::Runtime => {
                // No window to create — tray is created in new_events(Init).
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.activation_window.as_mut() else { return };

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
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: UserEvent,
    ) {
        match event {
            #[cfg(windows)]
            UserEvent::TrayIconEvent(_e) => {
                // D-07: left-click is no-op in Phase 3
            }
            #[cfg(windows)]
            UserEvent::MenuEvent(e) => {
                self.handle_menu_event(event_loop, e);
            }
            UserEvent::HealthChanged(state) => {
                self.health = state;
                #[cfg(windows)]
                if let Some(rt) = &self.tray_runtime {
                    rt.apply_health(state);
                }
                // Suppress unused variable warning on Linux where tray_runtime field is absent.
                let _ = event_loop;
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        match self.mode {
            AppMode::Activation => {
                // Request a redraw every time the event loop is idle so egui can process
                // animations, pending repaints, and the oneshot result polling (Pattern 2).
                if let Some(window) = self.activation_window.as_ref() {
                    window.window().request_redraw();
                }
            }
            AppMode::Runtime => {
                // ControlFlow::Wait already set; no redraw loop needed.
                // The runtime is idle until a tray/menu/health event arrives.
            }
        }
    }
}

impl App {
    #[cfg(windows)]
    fn handle_menu_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        event: tray_icon::menu::MenuEvent,
    ) {
        let Some(rt) = &self.tray_runtime else { return };
        let items = rt.menu_items();

        if event.id == *items.reativar.id() {
            // "Reativar": open activation window inside the running event loop.
            self.is_reactivation = true;
            self.mode = AppMode::Activation;
            if self.activation_window.is_none() {
                match ActivationWindow::new(
                    event_loop,
                    self.rt.clone(),
                    self.http.clone(),
                    true,
                    self.app_dir.clone(),
                ) {
                    Ok(w) => self.activation_window = Some(w),
                    Err(e) => {
                        eprintln!("[brevly-print] Failed to create re-activation window: {e:#}");
                    }
                }
            }
        } else if event.id == *items.sobre.id() {
            tray_runtime::show_about_dialog();
        } else if event.id == *items.sair.id() {
            if tray_runtime::confirm_quit_dialog() {
                event_loop.exit();
            }
        }
        // status item is disabled; no action expected
    }
}

// ── main ─────────────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    // OQ3: Velopack bootstrapper MUST be the very first call in main() (Windows-only).
    // On Linux this is a no-op compile-time guard.
    #[cfg(windows)]
    velopack::VelopackApp::build().run();
    // (Non-Windows: no velopack bootstrapper call needed — the update flow is Windows-only.)

    // D-08/D-09: named mutex guard; second instance exits silently.
    // Placed after Velopack bootstrapper, before the tokio runtime build.
    #[cfg(windows)]
    let _mutex_guard = {
        use std::iter::once;
        let name: Vec<u16> = "Local\\BrevlyPrintAgent"
            .encode_utf16().chain(once(0)).collect();
        // SAFETY: Win32 FFI — pointer is derived from an owned Vec<u16> that remains alive
        // for the duration of this block. The mutex name uses Local\ for session-scoping.
        let result = unsafe {
            CreateMutexW(None, false, windows::core::PCWSTR(name.as_ptr()))
        };
        match result {
            Ok(handle) => {
                // SAFETY: Win32 FFI — GetLastError() is valid after CreateMutexW returns Ok.
                let last_err = unsafe { GetLastError() };
                if last_err == ERROR_ALREADY_EXISTS {
                    // SAFETY: Win32 FFI — handle is a valid HANDLE returned by CreateMutexW.
                    let _ = unsafe { CloseHandle(handle) };
                    return Ok(()); // silent exit — another instance is running
                }
                handle // hold for process lifetime
            }
            Err(_) => return Ok(()), // conservative: mutex failure → exit
        }
    };

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

    // Enable WAL mode on the main App connection (Pitfall 5 — the Pusher task
    // opens a second connection in WAL mode; both connections must agree on
    // journal_mode for concurrent writes to be safe).
    conn.pragma_update(None, "journal_mode", "WAL")
        .context("Failed to set WAL mode on main SQLite connection")?;

    // ── Credential check (ACT-07) ────────────────────────────────────────────
    // NotFound|Corrupt → activation window (first run or re-activation)
    // Ok(_)           → Runtime mode (already activated)
    // Io(e)           → fatal I/O error, propagate
    let cred = credential_store(&app_dir);
    // CR-02: capture result once to avoid TOCTOU — is_reactivation must reflect
    // the same call that determined needs_activation.
    let cred_result = cred.load();
    let needs_activation = match &cred_result {
        Ok(_token) => {
            // Already activated — start the tray runtime.
            println!("[brevly-print] Credential found — starting tray runtime.");
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
            return Err(anyhow::anyhow!("{e}")).context("Credential I/O error on startup");
        }
    };

    // Determine AppMode from credential check result (D-10: unified event loop).
    let mode = if needs_activation { AppMode::Activation } else { AppMode::Runtime };
    // D-02: seed Connected on successful startup; Phase 4 will drive real transitions.
    let health = HealthState::Connected;

    // ── winit event loop ─────────────────────────────────────────────────────
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .context("Failed to build winit event loop")?;
    event_loop.set_control_flow(ControlFlow::Wait);

    // Wire tray + menu event forwarding into the winit event loop BEFORE run_app().
    // Two separate proxies because each closure captures its own clone.
    // Pattern: src/main.rs user_event handler receives these as UserEvent variants.
    #[cfg(windows)]
    {
        let proxy = event_loop.create_proxy();
        TrayIconEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
        }));

        let proxy = event_loop.create_proxy();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = proxy.send_event(UserEvent::MenuEvent(event));
        }));
    }

    // ── Phase 4: Pusher task wiring (Runtime mode only) ──────────────────────
    //
    // We capture is_runtime BEFORE `mode` and `cred_result` are moved into App.
    // The Pusher spawn block requires an EventLoopProxy<UserEvent> (from create_proxy())
    // and must run after the event_loop is built but before run_app().
    let is_runtime = matches!(mode, AppMode::Runtime);

    // mpsc channel for Phase 4 → Phase 5 PrintEvent handoff (D-03).
    // The receiver is held in App._print_rx so the channel is not dropped;
    // Phase 5 will take it out of Option and consume it.
    let (print_tx, print_rx) = tokio::sync::mpsc::channel::<PrintEvent>(32);

    if is_runtime {
        // Read Pusher credentials from ConfigStore (D-01).
        // Treat missing or empty credentials as a hard error so operators can
        // diagnose misconfiguration immediately rather than seeing a perpetual
        // reconnect loop with generic WS errors (WR-01).
        let pusher_key = config_store::get(&conn, "pusher_key")
            .context("Failed to read pusher_key from ConfigStore")?
            .filter(|s| !s.is_empty())
            .context("pusher_key is missing from ConfigStore — re-activate to restore")?;
        let pusher_cluster = config_store::get(&conn, "pusher_cluster")
            .context("Failed to read pusher_cluster from ConfigStore")?
            .filter(|s| !s.is_empty())
            .context("pusher_cluster is missing from ConfigStore — re-activate to restore")?;
        let tenant_id = config_store::get(&conn, "tenant_id")
            .context("Failed to read tenant_id from ConfigStore")?
            .filter(|s| !s.is_empty())
            .context("tenant_id is missing from ConfigStore — re-activate to restore")?;
        let auth_url = config_store::get(&conn, "noren_base_url")
            .context("Failed to read noren_base_url from ConfigStore")?
            .unwrap_or_else(noren_base_url);

        let pusher_config = PusherConfig { key: pusher_key, cluster: pusher_cluster, tenant_id, auth_url };

        // Get agentToken from CredentialStore (D-02). On the Runtime path, cred_result is Ok.
        let agent_token = match &cred_result {
            Ok(bytes) => String::from_utf8(bytes.clone())
                .context("agentToken bytes are not valid UTF-8")?,
            Err(e) => unreachable!(
                "Runtime path requires Ok credential, but got: {e}"
            ),
        };

        // Health closure — Pusher task drives the tray via EventLoopProxy (C2).
        // Never touches tray-icon APIs directly (Pitfall 4).
        let proxy_for_pusher = event_loop.create_proxy();
        let send_health = move |state: HealthState| {
            let _ = proxy_for_pusher.send_event(UserEvent::HealthChanged(state));
        };

        let pusher_db_path = db_path.clone();
        let pusher_http = http.clone();
        let pusher_tx = print_tx.clone();
        rt_handle.spawn(async move {
            run_pusher_loop(pusher_config, agent_token, pusher_tx, send_health, pusher_db_path, pusher_http).await;
        });
        // Drop original sender — only pusher_tx (moved into the task) keeps the
        // channel open. When the Pusher task exits, rx.recv() returns None (Phase 5).
        drop(print_tx);
    }

    let mut app = App {
        rt: rt_handle,
        http,
        is_reactivation: matches!(cred_result, Err(CredentialError::Corrupt(_))),
        app_dir: app_dir.clone(),
        conn,
        mode,
        health,
        #[cfg(windows)]
        tray_runtime: None,
        activation_window: None,
        _print_rx: Some(print_rx),
    };
    event_loop.run_app(&mut app).context("Event loop error")?;

    // Keep runtime alive until process fully exits.
    drop(rt);
    Ok(())
}
