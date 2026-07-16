# Phase 3: Tray + Runtime + First Distributable — Research

**Researched:** 2026-07-16
**Domain:** winit 0.30 headless tray runtime, tray-icon 0.24, muda 0.19, windows 0.62 Win32 APIs, velopack vpk packaging, Authenticode signing CI
**Confidence:** HIGH (all key APIs read from installed crate source; Velopack directory structure verified from official docs; Win32 API signatures confirmed from crates.io source)

---

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

| Decision | Value |
|----------|-------|
| D-01/D-02 | Wire full `HealthState { Connected, Reconnecting, Problem }` machine now; seed `Connected` on successful startup |
| D-03 | Optional: set `Problem` if named printer absent at boot — best-effort, non-blocking; skip if adds complexity |
| D-04 | `HealthChanged(HealthState)` variant in `UserEvent`; all tray mutation on the event-loop thread via `EventLoopProxy` |
| D-05 | Minimal solid-color RGBA icons (green/yellow/red), 16×16 source, embedded via `include_bytes!` |
| D-06 | Right-click menu: disabled status line + "Reativar impressora/licença" + "Sobre" (MessageBoxW) + "Sair" (MessageBoxW confirm) |
| D-07 | Left-click on tray = no-op (planner discretion: optionally mirror "Sobre") |
| D-08/D-09 | Single-instance guard: `CreateMutexW("Local\\BrevlyPrintAgent")`, `GetLastError()==ERROR_ALREADY_EXISTS` → silent `exit(0)`; placed after Velopack bootstrapper, before runtime build |
| D-10 | Unify `main.rs`: event loop always built; `App` carries Activation mode (window) vs Runtime mode (tray, no window, `ControlFlow::Wait`); "Reativar" recreates `ActivationWindow` in-loop |
| D-11 | Tokio runtime stays alive for whole process (already the case) |
| D-12/D-13/D-14 | `vpk pack` produces `Setup.exe`; `signtool` step in CI gated on OV cert secret; self-signed dev installer proves the loop now; SmartScreen reality documented |

### Claude's Discretion (Delegated)
- Exact tray icon rendering (embedded PNG vs programmatic RGBA circles)
- In-loop window recreation vs exit-relaunch for "Reativar"
- Whether to include boot-time printer-missing → red signal (D-03)
- Mutex scope name ergonomics
- CI job structure for `vpk pack` + conditional `signtool`
- Left/double-click tray behavior

### Deferred Ideas (OUT OF SCOPE)
- Real connection signal (Pusher) → Phase 4
- Windows toast notifications → Phase 6
- Auto-update download/apply (DIST-02/03) → Phase 7
- Branded tray artwork → future UI pass
- Boot-crash job recovery (RES-04) → Phase 6
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| RUN-01 | Agent runs invisibly; no UI beyond tray icon during normal operation | §Headless Tray + winit 0.30: `ControlFlow::Wait` with zero windows; D-10 App mode unification |
| RUN-02 | Tray icon reflects state: green=connected, yellow=reconnecting, red=problem | §HealthState machine; `TrayIcon::set_icon()` + `set_tooltip()` API confirmed from source |
| RUN-03 | Survives reboot; auto-reconnects without intervention | §Velopack+autostart integration: HKCU Run points to stable stub exe, not `current/` |
| DIST-01 | Installer is Authenticode-signed from first release | §vpk pack + signtool CI; self-signed dev flow; OV cert is external blocker |
</phase_requirements>

---

## Summary

Phase 3 turns the existing activated-but-headless branch point into a real always-on tray agent. Four implementation domains need precision: (1) the headless tray-icon + winit 0.30 integration — proven by the tray-icon 0.24 source's own `lib.rs` docstring example, (2) the Windows single-instance mutex using the `windows` 0.62 crate, (3) the Velopack install directory structure and how the HKCU Run autostart entry must target the root-level stub exe (not `current/`), and (4) the `vpk pack` command and conditional `signtool` CI step.

The most critical new finding is the **Velopack + auto-launch path integration**: Velopack installs to `%LocalAppData%\{packId}\current\brevly-print.exe` but creates a **root-level stub** at `%LocalAppData%\{packId}\brevly-print.exe` that survives updates. The HKCU Run entry written by `auto-launch` at activation time must point at this stub, not the `current\` path. Since `auto-launch` in Phase 2 registered the path of `std::env::current_exe()` at save time, and the first launch during testing is the dev binary (not the Velopack-installed stub), there is a real integration risk: the dev binary path will be wrong on a Velopack-installed system. The plan must include an explicit step that either: (a) uses `velopack`'s locator to discover the stub path at activation time, or (b) registers the path of the root-level stub (by looking one level up from `current_exe()`). This is the KEY autostart/Velopack integration risk for Phase 3.

**Primary recommendation:** Implement in this order — (1) health-state machine + HealthState enum, (2) headless tray creation in `new_events(StartCause::Init)`, (3) right-click menu with `muda` items, (4) single-instance mutex guard, (5) on-demand "Reativar" window, (6) `vpk pack` CI step + conditional `signtool`.

---

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Tray icon creation & mutation | Main OS thread (Win32 event loop) | — | `tray-icon` requires Win32 message pump; must be created after `StartCause::Init`; `!Send` (Rc-wrapped) |
| HealthState transitions | Main OS thread (via `UserEvent`) | Tokio thread pool (push via `EventLoopProxy`) | D-04: all tray mutation on event-loop thread; background tasks push `HealthChanged` via proxy |
| Right-click menu event dispatch | Main OS thread (`user_event` handler) | — | `muda::MenuEvent` forwarded via `MenuEvent::set_event_handler` + proxy |
| Single-instance mutex | Main thread, pre-event-loop | — | Must check before building tokio runtime and event loop; held for process lifetime |
| MessageBoxW dialogs | Main OS thread | — | Win32 modal dialog; blocks until dismissed; safe on event loop thread |
| Velopack packaging (`vpk pack`) | CI (Windows runner) | — | Post-build step; not a runtime concern |
| Authenticode signing (`signtool`) | CI (Windows runner) | — | Applied to `Setup.exe`; gated on secret availability |
| HKCU Run autostart registration | Phase 2 (`auto-launch`) | — | Already done at save time; Phase 3 validates path points at Velopack stub |

---

## Standard Stack

No new crates are added in Phase 3. All dependencies are already in `Cargo.toml`. This phase activates the already-present Windows-only crates.

### Active in Phase 3

| Crate | Version (locked) | Feature Flags Needed | Phase 3 Use |
|-------|-----------------|---------------------|-------------|
| `tray-icon` | 0.24.1 | (no additional; already in `[target.'cfg(windows)'.dependencies]`) | Tray icon creation, icon swap, tooltip, right-click menu |
| `muda` | 0.19.3 | (transitive dep of tray-icon; re-exported as `tray_icon::menu`) | `MenuItem`, `Menu`, `MenuEvent`, `PredefinedMenuItem` for the right-click menu |
| `windows` | 0.62.2 | `Win32_System_Threading` + `Win32_UI_WindowsAndMessaging` (must ADD to Cargo.toml) | `CreateMutexW`, `GetLastError`, `ERROR_ALREADY_EXISTS`, `MessageBoxW`, `MB_YESNO`, `IDYES`, `MB_ICONQUESTION` |
| `velopack` | 1.2.0 | (already present) | Bootstrapper already wired; Phase 3 adds `vpk pack` CI step |
| `auto-launch` | 0.6.0 | (already present; Windows-only) | Path-target verification: must point at Velopack root stub |

### New `windows` Feature Flags (add to existing entry in Cargo.toml)

```toml
[target.'cfg(windows)'.dependencies.windows]
version = "0.62"
features = [
    "Win32_Graphics_Printing",      # Phase 5 (already there)
    "Win32_Foundation",             # already there
    "Win32_System_Threading",       # NEW Phase 3: CreateMutexW, GetLastError
    "Win32_UI_WindowsAndMessaging", # NEW Phase 3: MessageBoxW, MB_YESNO, IDYES, MB_ICONQUESTION
]
```

[VERIFIED: windows 0.62.2 Cargo.toml feature flags confirmed in `/home/zephyr/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/windows-0.62.2/Cargo.toml` lines 650, 707]

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Programmatic RGBA circles | Embedded PNG/ICO | PNG requires `image` crate decode; programmatic RGBA is zero-dep and guaranteed-correct dimensions |
| `MessageBoxW` for dialogs | `egui` modal inside event loop | `MessageBoxW` is 3 lines vs full egui modal implementation; appropriate for one-line info/confirm |
| Manual Win32 HANDLE hold | `OwnedHandle` wrapper | Raw `HANDLE` from `CreateMutexW` does not auto-close (no `Drop` impl confirmed); holding it in `main()` scope achieves the same without wrapper overhead |

---

## Package Legitimacy Audit

No new packages are installed in Phase 3. All packages were audited in Phase 1. `muda` 0.19.3 is a transitive dependency of `tray-icon` 0.24.1 (re-exported as `tray_icon::menu`), not a direct dependency.

| Package | Registry | Age | Status | Disposition |
|---------|----------|-----|--------|-------------|
| `tray-icon` | crates.io | 3+ yrs | [VERIFIED Phase 1] | Approved |
| `muda` (transitive) | crates.io | 3+ yrs | [VERIFIED: crates.io] — github.com/tauri-apps/muda | Approved |
| `windows` | crates.io | 4+ yrs | [VERIFIED Phase 1] | Approved |
| `velopack` | crates.io | active | [VERIFIED Phase 1] | Approved |
| `auto-launch` | crates.io | 3+ yrs | [VERIFIED Phase 1] | Approved |

**Packages removed due to slopcheck:** none — no new packages in Phase 3.

---

## Architecture Patterns

### System Architecture Diagram — Phase 3 Runtime

```
 main() startup order
   1. VelopackApp::build().run()         ← must be first (OQ3)
   2. CreateMutexW("Local\BrevlyPrint")  ← if ERROR_ALREADY_EXISTS → exit(0)
   3. build tokio runtime                ← multi-thread, keep alive
   4. build reqwest::Client
   5. init_app_dir() / open_and_migrate()
   6. credential probe
       ├─ needs_activation=true  → App { mode: Activation }
       └─ needs_activation=false → App { mode: Runtime, health: Connected }

 EventLoop::<UserEvent>::with_user_event()
   set_control_flow(ControlFlow::Wait)     ← idle until event; no busy loop
   create_proxy() → TrayIconEvent::set_event_handler(proxy)
                  → MenuEvent::set_event_handler(proxy)
   run_app(&mut App)

 ApplicationHandler:
   new_events(StartCause::Init)     ← ONLY safe tray creation point
     if Runtime mode:
       build Menu (muda)
       TrayIconBuilder::new()
         .with_icon(green_icon)
         .with_tooltip("Conectado")
         .with_menu(menu)
         .build()

   resumed()
     if Activation mode: ActivationWindow::new()  ← existing Phase 2 code
     if Runtime mode:    nothing (no window)

   window_event()
     if Activation mode: forward to ActivationWindow  ← existing Phase 2 code
     if Runtime mode:    no-op (no window to receive events)

   user_event(UserEvent::TrayIconEvent(e))
     Left click → no-op (D-07)

   user_event(UserEvent::MenuEvent(e))
     match e.id():
       "reativar" → create ActivationWindow in-loop (D-10)
       "sobre"    → MessageBoxW(version string)
       "sair"     → MessageBoxW(confirm) → if IDYES: event_loop.exit()

   user_event(UserEvent::HealthChanged(state))
     tray.set_icon(icon_for(state))
     tray.set_tooltip(tooltip_for(state))
     status_item.set_text(label_for(state))  ← muda MenuItem::set_text

   about_to_wait()
     if Activation mode: window.request_redraw()   ← existing
     if Runtime mode:    nothing (ControlFlow::Wait handles idle)

 Background tasks (Phase 4+):
   proxy.send_event(UserEvent::HealthChanged(Reconnecting))
```

### Recommended Project Structure

```
src/
├── main.rs              # restructured: App { mode, health, tray, menu_items, ... }
├── tray_runtime.rs      # #[cfg(windows)] TrayRuntime: icon creation, menu, health update
├── health_state.rs      # HealthState enum + icon/tooltip/label mapping (portable)
├── activation_window.rs # unchanged Phase 2
├── activation_state.rs  # unchanged Phase 2
├── config_store.rs      # unchanged
├── credential_store/    # unchanged
├── printer/             # unchanged Phase 2
├── app_dir.rs           # unchanged
└── assets/
    ├── tray_green.rgba  # 16×16 RGBA raw bytes (64 bytes, generated once)
    ├── tray_yellow.rgba # 16×16 RGBA raw bytes
    └── tray_red.rgba    # 16×16 RGBA raw bytes
```

**Notes:**
- `health_state.rs` carries only the enum + pure mappings (string labels, filename selection) — portable, testable on Linux.
- `tray_runtime.rs` is `#[cfg(windows)]` and holds the `tray-icon` + `muda` logic.
- `assets/` holds the raw 16×16 RGBA byte arrays. These can be generated at build time or committed as static files. The simplest approach: a `build.rs` that generates `{color}_16x16.rgba` once, or inline them as const arrays in `health_state.rs`.

---

### Pattern 1: Headless tray creation — the correct timing

**What:** `TrayIconBuilder::build()` must be called AFTER the winit event loop's Win32 message pump is running. The canonical safe point is inside `ApplicationHandler::new_events()` when `cause == StartCause::Init`.

**Critical:** Do NOT call `TrayIconBuilder::build()` before `run_app()` — the Win32 message pump (needed to process `WM_TRAYNOTIFY`) is not yet running. The tray icon will be silently invisible or panic.

**Source:** [VERIFIED: tray-icon 0.24.1 `src/lib.rs` docstring, lines 18–19: "an event loop must be running on the thread, on Windows, a win32 event loop… you must make sure that the event loop is already running and not just created before creating a TrayIcon… the earliest you can create icons is on `StartCause::Init`"]

```rust
// Source: tray-icon 0.24.1 src/lib.rs verified pattern
// Confirmed identical to Phase 1's 01-RESEARCH.md Pattern 1

impl ApplicationHandler<UserEvent> for App {
    fn new_events(
        &mut self,
        _event_loop: &ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        #[cfg(windows)]
        if cause == winit::event::StartCause::Init {
            if matches!(self.mode, AppMode::Runtime) {
                // Build right-click menu first (needed for TrayIconBuilder)
                let menu = build_tray_menu(&self.menu_items);
                // Create tray icon
                let icon = self.health.icon();
                match TrayIconBuilder::new()
                    .with_icon(icon)
                    .with_tooltip(self.health.tooltip())
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(false)  // D-07: no menu on left click
                    .build()
                {
                    Ok(tray) => self.tray = Some(tray),
                    Err(e) => {
                        eprintln!("[brevly-print] Failed to create tray icon: {e}");
                        _event_loop.exit();
                    }
                }
            }
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        match self.mode {
            AppMode::Activation => {
                // Existing Phase 2 code: create ActivationWindow
                if self.window.is_none() { /* ... */ }
            }
            AppMode::Runtime => {
                // No window to create — tray is created in new_events(Init)
            }
        }
    }
}
```

---

### Pattern 2: EventLoopProxy wiring for tray + menu events

**What:** Before `run_app()`, set event handlers that forward tray and menu events into the winit event loop. Two `create_proxy()` calls are needed because each closure captures its own proxy.

**Source:** [VERIFIED: tray-icon 0.24.1 `src/lib.rs` lines 106–117, the "Note for winit users" section with exact code]

```rust
// Source: tray-icon 0.24.1 src/lib.rs (confirmed from installed crate source)
use tray_icon::{TrayIconEvent, menu::MenuEvent};

// Wire BEFORE run_app():
let proxy = event_loop.create_proxy();
TrayIconEvent::set_event_handler(Some(move |event| {
    let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
}));

let proxy = event_loop.create_proxy();
MenuEvent::set_event_handler(Some(move |event| {
    let _ = proxy.send_event(UserEvent::MenuEvent(event));
}));
```

**Important:** `set_event_handler` takes `Option<F>` where `F: Fn(TrayIconEvent) + Send + Sync + 'static`. The closure must be `Send + Sync` because it may be called from the Win32 message thread. The `EventLoopProxy::send_event` is `Send`, so capturing `proxy` in the closure satisfies this.

---

### Pattern 3: HealthState machine + icon construction

**What:** `HealthState` is a portable enum. Icon creation uses `tray_icon::Icon::from_rgba(Vec<u8>, width, height)`.

**Source:** [VERIFIED: tray-icon 0.24.1 `src/icon.rs` lines 131–139: `Icon::from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Self, BadIcon>`]

```rust
// health_state.rs — portable (no #[cfg(windows)] needed for the enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Connected,      // green
    Reconnecting,   // yellow
    Problem,        // red
}

impl HealthState {
    pub fn tooltip(&self) -> &'static str {
        match self {
            Self::Connected    => "Brevly Print — Conectado",
            Self::Reconnecting => "Brevly Print — Reconectando…",
            Self::Problem      => "Brevly Print — Problema de conexão",
        }
    }

    pub fn status_label(&self) -> &'static str {
        match self {
            Self::Connected    => "Conectado",
            Self::Reconnecting => "Reconectando…",
            Self::Problem      => "Problema de conexão",
        }
    }
}

// Icon construction — Windows-only (tray-icon is Windows-only dep)
#[cfg(windows)]
impl HealthState {
    pub fn icon(&self) -> tray_icon::Icon {
        // 16×16 solid-color RGBA bytes, embedded at compile time
        // Each is exactly 16*16*4 = 1024 bytes
        let (rgba_bytes, w, h): (&[u8], u32, u32) = match self {
            Self::Connected    => (include_bytes!("../assets/tray_green.rgba"),  16, 16),
            Self::Reconnecting => (include_bytes!("../assets/tray_yellow.rgba"), 16, 16),
            Self::Problem      => (include_bytes!("../assets/tray_red.rgba"),    16, 16),
        };
        tray_icon::Icon::from_rgba(rgba_bytes.to_vec(), w, h)
            .expect("tray icon RGBA bytes are always valid")
    }
}
```

**Generating the RGBA asset files** (run once in a build script or at dev setup):

```rust
// build.rs — generates solid-color 16x16 RGBA files
// Each pixel = [R, G, B, A] × 256 pixels
fn write_solid(path: &str, r: u8, g: u8, b: u8) {
    let mut bytes = Vec::with_capacity(16 * 16 * 4);
    for _ in 0..(16 * 16) {
        bytes.extend_from_slice(&[r, g, b, 255]);
    }
    std::fs::write(path, &bytes).unwrap();
}
// write_solid("src/assets/tray_green.rgba",  0x22, 0xC5, 0x5E);  // #22C55E
// write_solid("src/assets/tray_yellow.rgba", 0xF5, 0x9E, 0x0B);  // #F59E0B
// write_solid("src/assets/tray_red.rgba",    0xEF, 0x44, 0x44);  // #EF4444
```

**Icon swap on health change** (`tray_icon::TrayIcon::set_icon`):

```rust
// Source: tray-icon 0.24.1 src/lib.rs line 387: pub fn set_icon(&self, icon: Option<Icon>) -> Result<()>
if let Some(tray) = &self.tray {
    let _ = tray.set_icon(Some(health.icon()));
    let _ = tray.set_tooltip(Some(health.tooltip()));
    // Also update the status label in the menu
    self.menu_items.status.set_text(health.status_label());
}
```

---

### Pattern 4: Right-click menu construction with muda

**What:** The menu is built using `muda` (re-exported as `tray_icon::menu`). The status line is a disabled `MenuItem`. Menu items have stable IDs used for dispatch in `user_event`.

**Source:** [VERIFIED: muda 0.19.3 `src/items/normal.rs` — `MenuItem::new(text, enabled, accel)`, `set_text()`, `set_enabled()` all confirmed; `src/lib.rs` shows `Menu::new()`, `menu.append()`]

```rust
// #[cfg(windows)]
use tray_icon::menu::{Menu, MenuItem, MenuEvent, PredefinedMenuItem};

pub struct TrayMenuItems {
    pub status: MenuItem,      // disabled status line
    pub reativar: MenuItem,
    pub sobre: MenuItem,
    pub sair: MenuItem,
}

pub fn build_tray_menu(health: HealthState) -> (Menu, TrayMenuItems) {
    let status = MenuItem::new(health.status_label(), false, None); // false = disabled
    let reativar = MenuItem::new("Reativar impressora/licença", true, None);
    let sobre = MenuItem::new("Sobre", true, None);
    let sair = MenuItem::new("Sair", true, None);

    let menu = Menu::new();
    menu.append(&status).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&reativar).unwrap();
    menu.append(&sobre).unwrap();
    menu.append(&PredefinedMenuItem::separator()).unwrap();
    menu.append(&sair).unwrap();

    let items = TrayMenuItems { status, reativar, sobre, sair };
    (menu, items)
}

// In user_event for UserEvent::MenuEvent(e):
fn handle_menu_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    e: tray_icon::menu::MenuEvent,
) {
    let items = &self.menu_items;
    if e.id == *items.reativar.id() {
        self.open_reactivation_window(event_loop);
    } else if e.id == *items.sobre.id() {
        self.show_about_dialog();
    } else if e.id == *items.sair.id() {
        self.confirm_quit(event_loop);
    }
}
```

---

### Pattern 5: Single-instance mutex guard

**What:** `CreateMutexW` is called with a per-session name (`Local\`). When a second instance calls it, Windows returns a valid HANDLE but also sets the thread's last-error to `ERROR_ALREADY_EXISTS` (183). The `windows` 0.62 crate wraps `CreateMutexW` to return `Result<HANDLE>` — `Ok(handle)` is returned even when the mutex already existed. Must check `GetLastError` after the successful call.

**Critical HANDLE lifetime:** `HANDLE` returned by `CreateMutexW` does NOT implement `Drop` (it is a plain newtype over `isize`). The OS releases the mutex when the process exits. To be explicit and avoid confusion, store the HANDLE in a local variable that lives for the entire `main()` function scope. Do not wrap in `Owned<HANDLE>` unless you want `CloseHandle` called on drop (which would release the mutex and allow a second instance to start).

**Source:** [VERIFIED: windows 0.62.2 `src/Windows/Win32/System/Threading/mod.rs` — `CreateMutexW` signature confirmed; `src/Windows/Win32/Foundation/mod.rs` — `GetLastError()` and `ERROR_ALREADY_EXISTS = WIN32_ERROR(183u32)` confirmed]

```rust
// #[cfg(windows)]
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
    System::Threading::CreateMutexW,
};

/// Returns the mutex HANDLE that must be kept alive for process lifetime.
/// Returns None if another instance is already running (silently exit).
///
/// Safety: called once at process start; name is a string literal.
#[cfg(windows)]
fn try_acquire_single_instance() -> Option<HANDLE> {
    // WR-06: null-terminated UTF-16 via .chain(once(0))
    use std::iter::once;
    let name: Vec<u16> = "Local\\BrevlyPrintAgent"
        .encode_utf16()
        .chain(once(0))
        .collect();
    let result = unsafe {
        CreateMutexW(
            None,   // default security attributes
            false,  // binitialowner: we don't need to own it
            windows::core::PCWSTR(name.as_ptr()),
        )
    };
    match result {
        Ok(handle) => {
            // CreateMutexW can succeed AND report ERROR_ALREADY_EXISTS
            let last_err = unsafe { GetLastError() };
            if last_err == ERROR_ALREADY_EXISTS {
                // Another instance is running — close our handle and signal caller to exit
                let _ = unsafe { CloseHandle(handle) };
                None  // caller should exit(0)
            } else {
                Some(handle)  // we are the first instance; hold HANDLE for process lifetime
            }
        }
        Err(_) => {
            // CreateMutexW failed entirely (unusual); proceed single-instance
            // (better than refusing to start due to a mutex error)
            None  // conservative: exit to avoid undefined behavior
        }
    }
}

// In main(), after VelopackApp::build().run():
#[cfg(windows)]
let _mutex_guard = match try_acquire_single_instance() {
    Some(h) => h,
    None => {
        return Ok(()); // silent exit — another instance is running
    }
};
// _mutex_guard lives until end of main() — holds the mutex for process lifetime
```

**Why `Local\` not `Global\`?** `Local\` namespaces the mutex to the current login session. Since the agent is a per-user HKCU autostart app, two different Windows users each get their own agent instance (correct). `Global\` would prevent that. [CITED: Microsoft Win32 Kernel Object Namespaces documentation]

---

### Pattern 6: MessageBoxW for "Sobre" and "Sair" confirm

**Source:** [VERIFIED: windows 0.62.2 `src/Windows/Win32/UI/WindowsAndMessaging/mod.rs` — `MessageBoxW` signature, `IDYES`, `MB_YESNO`, `MB_ICONQUESTION` all confirmed]

```rust
// #[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
    IDYES, MESSAGEBOX_RESULT,
};

/// WR-06: build null-terminated UTF-16 string from &str
fn to_wstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn show_about_dialog() {
    let version = env!("CARGO_PKG_VERSION");
    let text = to_wstr(&format!(
        "Brevly Print v{}\n\nAgente de impressão para Noren.",
        version
    ));
    let caption = to_wstr("Sobre o Brevly Print");
    unsafe {
        MessageBoxW(
            None, // no parent HWND
            windows::core::PCWSTR(text.as_ptr()),
            windows::core::PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(windows)]
fn confirm_quit() -> bool {
    let text = to_wstr(
        "Fechar o Brevly Print?\nAs impressões vão parar enquanto o programa estiver fechado."
    );
    let caption = to_wstr("Brevly Print — Sair");
    let result: MESSAGEBOX_RESULT = unsafe {
        MessageBoxW(
            None,
            windows::core::PCWSTR(text.as_ptr()),
            windows::core::PCWSTR(caption.as_ptr()),
            MB_YESNO | MB_ICONQUESTION,
        )
    };
    result == IDYES
}
```

**Note:** The `MB_OK`, `MB_YESNO`, `MB_ICONQUESTION`, `MB_ICONINFORMATION` constants are of type `MESSAGEBOX_STYLE` and support bitwise OR via `|`. The return value `MESSAGEBOX_RESULT` compares with `IDYES` (value 6). [VERIFIED from source]

---

### Pattern 7: On-demand window recreation for "Reativar"

**What:** When "Reativar" is selected from the tray menu, create an `ActivationWindow` on the running event loop. After save, the window exits via the existing `should_exit()` / `event_loop.exit()` path in `window_event(CloseRequested)`. Since Phase 2 already exits the process on save (D-15), the simplest path for "Reativar" is to allow the existing exit path to fire — the next launch (which happens immediately due to Velopack's on-restarted hook or the user double-clicking the tray after manual close) comes up fresh.

**In-loop approach (preferred per D-10):** `event_loop.create_window()` inside `window_event` or triggered from `user_event`. Store the resulting `ActivationWindow` in `self.window`. On save/close, drop it and set `self.window = None`. The tray stays alive throughout.

**Pitfall:** `wgpu::Surface` is tied to the window. When `ActivationWindow` is dropped, its wgpu surface is destroyed. Creating a new one inside the same event loop is clean on winit 0.30 provided the previous surface is fully dropped first. Use `Option<ActivationWindow>` and always drop before creating. [ASSUMED — based on winit 0.30 design principles; Phase 3 plan should include a smoke test of in-loop window create/destroy]

```rust
// AppMode tracks whether we are in activation or runtime state
enum AppMode {
    Activation { window: Option<ActivationWindow> },
    Runtime { tray: Option<TrayIcon>, menu_items: TrayMenuItems },
}

// In user_event, when "Reativar" is clicked:
fn open_reactivation_window(&mut self, event_loop: &ActiveEventLoop) {
    if let AppMode::Runtime { .. } = &self.mode {
        match ActivationWindow::new(event_loop, self.rt.clone(), self.http.clone(), true, self.app_dir.clone()) {
            Ok(w) => {
                // Switch mode; tray stays alive (still in self)
                self.reactivation_window = Some(w);
            }
            Err(e) => eprintln!("[brevly-print] Failed to open reactivation window: {e:#}"),
        }
    }
}
```

---

### Pattern 8: Velopack packaging and autostart path integration

**What:** `vpk pack` produces `{packId}-Setup.exe`. The installed directory structure is:

```
%LocalAppData%\{packId}\
    brevly-print.exe      ← ROOT STUB (stable path that survives updates)
    Update.exe            ← Velopack updater
    packages\             ← downloaded update packages
    current\
        brevly-print.exe  ← REAL executable (replaced on each update)
        sq.version        ← Velopack manifest
```

**KEY INTEGRATION RISK:** `auto-launch` in Phase 2 registers `std::env::current_exe()` as the HKCU Run path. At activation time, if the agent is running from the `current\` directory (Velopack-installed), `current_exe()` returns `%LocalAppData%\{packId}\current\brevly-print.exe` — but after a Velopack update, `current\` is replaced and the path in HKCU Run points to the new exe. The stub at the root level (`%LocalAppData%\{packId}\brevly-print.exe`) is the stable path.

**Resolution:** At activation save time, detect whether running inside a Velopack install by checking if `Update.exe` exists in the parent of `current_exe()`. If yes, register the parent directory's `brevly-print.exe` (the stub) as the autostart path, not `current_exe()` itself.

```rust
// src/activation_state.rs or auto_launch helper
fn resolve_autostart_exe_path() -> std::path::PathBuf {
    let current_exe = std::env::current_exe().unwrap();
    let parent = current_exe.parent().unwrap();
    // If Update.exe exists in the same directory, we ARE in a Velopack install
    // (running from current/) — register the stub in the parent.
    // Velopack installs: %LocalAppData%\{packId}\current\<exe> → parent is 'current'
    // and parent.parent() is %LocalAppData%\{packId}\ which contains Update.exe
    if let Some(grandparent) = parent.parent() {
        if grandparent.join("Update.exe").exists() {
            // Running inside Velopack 'current' dir — use the root stub
            return grandparent.join(current_exe.file_name().unwrap());
        }
    }
    // Running from parent dir (dev build or non-Velopack install) — use current_exe
    current_exe
}
```

[VERIFIED: Velopack directory structure confirmed from docs.velopack.io/packaging/operating-systems/windows — "root-level YourApp.exe is a small execution stub… shortcuts/launchers can point at a stable path that survives updates"; Velopack locator source confirms `CurrentBinaryDir: root_dir.join("current")`]

**`vpk pack` command syntax:**

```bash
# Install vpk CLI (Windows runner CI step)
dotnet tool install -g vpk

# Package after cargo build --release
vpk pack \
  --packId BrevlyPrint \
  --packVersion 0.1.0 \
  --packDir target/release \
  --mainExe brevly-print.exe \
  --outputDir Releases \
  --packTitle "Brevly Print" \
  --packAuthors "Brevly"
```

[VERIFIED: docs.velopack.io/reference/cli/content/vpk-windows — `--packId`, `--packVersion`, `--packDir`, `--mainExe` all confirmed as required/key parameters]

**Note:** `--packVersion` must be a semver-compatible version string. Sync with `Cargo.toml` version. Consider a CI step that reads `cargo metadata` to extract the version automatically.

---

### Pattern 9: Authenticode signing CI step (conditional on secret)

**What:** Add a Windows CI step after `vpk pack` that signs the `Setup.exe` using `signtool.exe` (available on GitHub Actions `windows-latest` as part of the Windows SDK). The step is gated on the `CODESIGN_PFX_BASE64` secret being non-empty.

**Source:** [CITED: GitHub Actions conditional expression docs; `vpk` docs confirming `--signParams` flag]

```yaml
# Addition to .github/workflows/ci.yml — Windows job

    - name: Build release binary
      run: cargo build --release --target x86_64-pc-windows-msvc

    - name: Install vpk CLI
      run: dotnet tool install -g vpk

    - name: Package with vpk
      run: |
        $version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
        vpk pack `
          --packId BrevlyPrint `
          --packVersion $version `
          --packDir target\release `
          --mainExe brevly-print.exe `
          --outputDir Releases `
          --packTitle "Brevly Print" `
          --packAuthors "Brevly"
      shell: pwsh

    # Sign only when the OV cert secret is present.
    # When the secret is absent (e.g., PRs from forks, pre-cert), this step is skipped cleanly.
    - name: Sign Setup.exe (Authenticode — OV cert)
      if: ${{ secrets.CODESIGN_PFX_BASE64 != '' }}
      shell: pwsh
      env:
        CODESIGN_PFX_BASE64: ${{ secrets.CODESIGN_PFX_BASE64 }}
        CODESIGN_PFX_PASSWORD: ${{ secrets.CODESIGN_PFX_PASSWORD }}
      run: |
        # Decode PFX from base64 secret
        $pfxBytes = [Convert]::FromBase64String($env:CODESIGN_PFX_BASE64)
        $pfxPath = "$env:RUNNER_TEMP\cert.pfx"
        [IO.File]::WriteAllBytes($pfxPath, $pfxBytes)

        # Sign Setup.exe with SHA-256 + RFC3161 timestamp
        $setupExe = Get-ChildItem -Path Releases -Filter "*Setup.exe" | Select-Object -First 1
        & signtool sign `
          /fd SHA256 `
          /f "$pfxPath" `
          /p "$env:CODESIGN_PFX_PASSWORD" `
          /tr http://timestamp.digicert.com `
          /td SHA256 `
          "$($setupExe.FullName)"

        # Clean up PFX from temp
        Remove-Item $pfxPath

    - name: Upload Setup.exe artifact
      uses: actions/upload-artifact@v4
      with:
        name: brevly-print-setup
        path: Releases/*Setup.exe
```

**Note:** `vpk` also supports `--signParams "/fd SHA256 /f cert.pfx ..."` to have it sign files during packaging. However, calling `signtool` separately after `vpk pack` is simpler for the gated-on-secret pattern and keeps the signing step explicit and auditable.

---

### Pattern 10: Self-signed dev certificate for testing the signed install flow

**What:** Generate a self-signed code-signing cert on the Windows dev machine or CI to test the entire `vpk pack → signtool → install → autostart → tray` loop without the real OV cert.

**Source:** [CITED: Microsoft Learn — New-SelfSignedCertificate PowerShell cmdlet]

```powershell
# Run once on Windows to create a test cert
# This cert is NOT trusted by Windows SmartScreen or users — dev/CI testing only

# 1. Create self-signed code signing cert
$cert = New-SelfSignedCertificate `
    -Subject "CN=BrevlyPrint Dev, O=Brevly Dev, C=BR" `
    -Type CodeSigning `
    -CertStoreLocation Cert:\CurrentUser\My `
    -HashAlgorithm SHA256

# 2. Export as PFX for signtool
$pfxPassword = ConvertTo-SecureString -String "devtest123" -Force -AsPlainText
Export-PfxCertificate `
    -Cert $cert `
    -FilePath "$HOME\brevly-dev-cert.pfx" `
    -Password $pfxPassword

# 3. Sign with signtool (same command as production, minus real timestamp)
signtool sign /fd SHA256 /f "$HOME\brevly-dev-cert.pfx" /p devtest123 Releases\BrevlyPrint-Setup.exe

# 4. Install: Windows will show "Unknown publisher" (self-signed; not OV-trusted)
#    Click "More info → Run anyway" for the test install
#    Verify: reboot → tray appears → all Phase 3 success criteria except SC-4
```

**SmartScreen reality (D-14):** Even a correctly OV-signed installer will show "Windows protected your PC" for the first ~weeks to months until the certificate accumulates download reputation. OV certs satisfy "no Unknown publisher" (SC-4); they do NOT skip the SmartScreen reputation warmup. This is expected behavior, not a bug.

---

### Anti-Patterns to Avoid

- **Creating `TrayIcon` before `StartCause::Init`:** The Win32 message pump is not yet running. The tray notification window (hidden `WM_TRAYNOTIFY` receiver) requires the pump. Create only inside `new_events(StartCause::Init)`.
- **Registering HKCU Run with `current_exe()` directly inside a Velopack install:** Points to `current\brevly-print.exe`, which gets replaced on update. The registered path becomes a dangling pointer. Always check for `Update.exe` in parent dir and register the root stub instead.
- **Storing the mutex HANDLE in an `Owned<HANDLE>`:** `Owned<T: Free>` calls `T::free()` on drop. If `HANDLE` implements `Free` via `CloseHandle`, the mutex is released, allowing a second instance to acquire it before the first process exits. Keep the raw `HANDLE` in a plain variable for process lifetime.
- **Making the `TrayIconEvent` handler blocking:** The handler is called from the Win32 message thread. Heavy work inside it will freeze the tray. The handler must only call `proxy.send_event()` and return immediately.
- **Using `ControlFlow::Poll` in Runtime mode:** The agent is idle 99.9% of the time. `ControlFlow::Poll` would busy-loop and peg a CPU core. Use `ControlFlow::Wait` (already set in `main()`).
- **Calling `MessageBoxW` from a background thread:** `MessageBoxW` creates a modal window on the calling thread's message queue. Call only from the event loop thread (inside `user_event` handlers).

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tray icon right-click menu | Manual Win32 `CreatePopupMenu` + `TrackPopupMenu` | `muda 0.19` (via `tray_icon::menu`) | ~300 lines of Win32 boilerplate; muda handles menu creation, item state, accelerators, event routing |
| Tray event forwarding to winit | Manual Win32 `WM_TRAYNOTIFY` subclass | `TrayIconEvent::set_event_handler` + `EventLoopProxy` | tray-icon handles the hidden notification window and Win32 message routing |
| Windows installer packaging | Custom NSIS/Inno Setup script | `vpk pack` | Velopack handles: install dir, Update.exe deployment, root stub creation, shortcut creation, uninstall — consistent with Phase 7 auto-update toolchain |
| Wide string null termination | Custom encoding helpers | Established WR-06 pattern: `.encode_utf16().chain(once(0)).collect()` | Already validated in Phase 2 (`src/printer/spooler.rs`) — use the same pattern for Win32 PCWSTR |

**Key insight:** The Windows tray ecosystem has decades of Win32 gotchas (hidden notification windows, DPI-aware icon scaling, high-contrast mode). `tray-icon` 0.24 absorbs all of these. The only custom work is: (1) generating/embedding the three RGBA icon arrays, and (2) the menu item dispatch logic.

---

## Common Pitfalls

### Pitfall 1: TrayIcon created before Win32 message pump is running
**What goes wrong:** `TrayIconBuilder::build()` fails silently or panics — no tray icon appears, no error shown.
**Root cause:** The tray icon needs a hidden Win32 window (`Shell_NotifyIconW`) to receive `WM_TRAYNOTIFY` messages. This window can only be created on a thread with an active message pump.
**How to avoid:** Create `TrayIcon` exclusively inside `ApplicationHandler::new_events()` when `cause == StartCause::Init`. Never before `event_loop.run_app()`.
**Warning signs:** No tray icon visible; process running normally otherwise.

### Pitfall 2: HKCU Run points to `current\exe` instead of root stub
**What goes wrong:** After a Velopack update, the HKCU Run entry points to `%LocalAppData%\{packId}\current\brevly-print.exe`. Velopack replaces the `current\` directory, placing a new exe at the same path. This actually works for updates — BUT if the user uninstalls and reinstalls to a different app ID or version, the path can break. More critically: if Phase 2 ran from a dev build (not Velopack-installed), it registered the dev binary path — which completely fails after real install.
**Root cause:** `std::env::current_exe()` returns the actual running binary path, which inside a Velopack install is in `current\`. The root stub is what should be registered.
**How to avoid:** Pattern 8 — detect `Update.exe` in grandparent dir and register the grandparent's exe instead.
**Warning signs:** Reboot → no tray icon; `auto-launch.is_enabled()` returns true but exe doesn't appear in Task Manager startup.

### Pitfall 3: Second CreateMutexW result misinterpreted
**What goes wrong:** The `windows` 0.62 crate's `CreateMutexW` returns `Ok(handle)` when the mutex already exists (not `Err`). Checking only the `Result` variant will silently allow two instances to run.
**Root cause:** The Win32 `CreateMutexW` API distinguishes "mutex exists" via `GetLastError()`, not via the return value — a valid HANDLE is returned in both cases.
**How to avoid:** Always call `GetLastError()` immediately after `CreateMutexW` returns `Ok(_)` and check for `ERROR_ALREADY_EXISTS` (183).
**Warning signs:** Two tray icons visible; double Pusher subscriptions in Phase 4 causing double prints.

### Pitfall 4: Blocking the event loop thread with MessageBoxW
**What goes wrong:** `MessageBoxW` is a blocking modal dialog. While it is open, the winit event loop is blocked. Tray icon events queued during this time are not processed. On Windows this is generally fine for short modal dialogs (the Win32 message pump inside `MessageBoxW` still runs), but it prevents any other `user_event` from firing until the dialog is dismissed.
**Root cause:** `MessageBoxW` runs its own nested message loop. Win32 delivers messages to it, but `ApplicationHandler::user_event` is not re-entrant.
**How to avoid:** Only call `MessageBoxW` from `user_event` (correct) — not from background tasks. Keep dialogs brief. Do not call `MessageBoxW` while an `ActivationWindow` is open (it would appear on top of a half-built window).
**Warning signs:** Tray appears unresponsive while a dialog is open (expected — this is correct Win32 behavior for modal dialogs).

### Pitfall 5: vpk version mismatch with Cargo.toml
**What goes wrong:** `vpk pack --packVersion 0.1.0` hardcoded in CI but `Cargo.toml` version differs. The installed Velopack manifest version does not match the binary's `CARGO_PKG_VERSION`. Auto-update logic in Phase 7 would break.
**Root cause:** Manual synchronization between `Cargo.toml` and the `vpk pack` command.
**How to avoid:** Extract version from `cargo metadata` in CI: `(cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version`.
**Warning signs:** `velopack::UpdateManager::get_current_version()` returns a different version than `env!("CARGO_PKG_VERSION")`.

---

## Runtime State Inventory

This phase is NOT a rename/refactor phase. Omit this section.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust `#[test]` + `cargo test` |
| Config file | `Cargo.toml` `[profile.test]` |
| Quick run command | `cargo test` (Linux — portable logic tests) |
| Full suite command | `cargo test --target x86_64-pc-windows-msvc` (Windows CI) |

### What is testable on Linux vs Windows

**Testable on Linux (portable logic):**

| Behavior | Test Type | Automated Command |
|----------|-----------|-------------------|
| RUN-02: `HealthState` enum variants and mappings (tooltip/label/icon filename) | unit | `cargo test health_state::tests` |
| RUN-02: `HealthState` transitions — all three states reachable | unit | `cargo test health_state::tests::transitions` |
| Menu action dispatch logic (match on item ID → action enum) | unit | `cargo test tray_runtime::tests::menu_dispatch` |
| D-08: `try_acquire_single_instance` logic (not the Win32 call, but the result handling) | unit (mock) | `cargo test single_instance::tests` |
| D-10: `AppMode` transitions (Activation → Runtime → Reactivation) | unit | `cargo test app_mode::tests` |

**Requires Windows / Manual verification:**

| Behavior | Verification Method |
|----------|---------------------|
| SC-1: Reboot → tray icon appears, no user action | Manual: install dev build, reboot, observe tray |
| SC-2: Tri-color tray state | Manual: trigger each `HealthState` via test command, observe icon color |
| SC-3: No open window during normal operation | Manual: observe no taskbar entry; Task Manager shows process but no window |
| SC-4: Signed installer — no "Unknown publisher" | Manual: install OV-signed `Setup.exe`, observe no SmartScreen block |
| D-06: Right-click menu appears, items work | Manual: right-click tray icon, verify each menu item |
| D-08: Single-instance guard | Manual: launch two instances, second one exits silently |
| D-09: Silent exit on second instance | Manual: observe no toast/dialog from second launch |
| RUN-03: Autostart survives Velopack update | Manual: install, update to v0.1.1, reboot, observe tray |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | Notes |
|--------|----------|-----------|-------------------|-------|
| RUN-01 | No visible window in Runtime mode | Manual | n/a | Verified by absence of taskbar entry |
| RUN-02 | `HealthState` transitions compile and map correctly | unit | `cargo test health_state::tests` | Automated on Linux |
| RUN-02 | Tray icon color changes on health state change | Manual | n/a | Windows-only visual |
| RUN-03 | Autostart registered and survives update | Manual | n/a | Velopack install required |
| DIST-01 | `vpk pack` produces `Setup.exe` | CI artifact check | Runs in Windows CI job | Automated in CI |
| DIST-01 | `signtool` signs when cert secret is present | CI job output | Conditional CI step | Requires OV cert secret |

### Sampling Rate
- **Per task commit:** `cargo test` on Linux (health state logic, menu dispatch)
- **Per wave merge:** `cargo test --target x86_64-pc-windows-msvc` in Windows CI
- **Phase gate:** All automated tests green + manual checklist from CONTEXT.md success criteria

### Wave 0 Gaps
- [ ] `src/health_state.rs` — `HealthState` enum, tooltip/label mappings, unit tests
- [ ] `src/assets/tray_green.rgba`, `tray_yellow.rgba`, `tray_red.rgba` — 16×16 RGBA files (or build.rs to generate)
- [ ] `src/tray_runtime.rs` — `#[cfg(windows)]` tray creation, menu build, icon swap (no tests for the Win32 code, but logic wrappers are testable)

---

## Environment Availability

| Dependency | Required By | Available (Linux dev) | Available (Windows CI) | Notes |
|------------|------------|----------------------|----------------------|-------|
| Rust stable | Build | ✓ | ✓ | |
| `tray-icon` 0.24 | Runtime tray | ✗ (cfg-gated, doesn't build/link on Linux) | ✓ | Windows-only dep |
| `muda` 0.19 | Right-click menu | ✗ (cfg-gated) | ✓ | Windows-only (transitive of tray-icon) |
| `windows` 0.62 + new features | Mutex, MessageBoxW | ✗ (cfg-gated) | ✓ | Windows-only |
| `vpk` CLI (dotnet tool) | Packaging | ✗ | ✓ (install via `dotnet tool install -g vpk`) | CI-only; not needed for dev build |
| `signtool.exe` | Signing | ✗ | ✓ (`windows-latest` includes Windows SDK) | CI-only; gated on OV cert secret |
| OV Authenticode cert | SC-4 | ✗ | ✗ (external blocker) | Tracked in STATE.md open todos |
| Windows machine (interactive) | Visual manual tests | ✗ | ✓ (via CI) / Owner's machine | For SC-1..SC-4 final verification |

**Missing dependencies with no fallback:**
- OV certificate: DIST-01 final sign-off blocked until procured. D-12 decision: self-signed dev cert proves the pipeline; OV cert is a gate for SC-4 only.

**Missing dependencies with fallback:**
- `vpk` not on dev machine: packaging only needed in CI; not blocking for code development.

---

## Security Domain

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Not in Phase 3 |
| V3 Session Management | No | Not in Phase 3 |
| V4 Access Control | Partial | Single-instance mutex prevents duplicate agent sessions (D-08) |
| V5 Input Validation | No | No user input in runtime mode; menu selections are enum-matched |
| V6 Cryptography | No | No crypto in Phase 3; DPAPI was Phase 1/2 |
| Code Integrity (DIST-01) | Yes | Authenticode OV cert + signtool; SHA-256 digest; RFC3161 timestamp |

### Threat Patterns

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Double-launch via HKCU autostart race | Elevation of Privilege / DoS | Named mutex `Local\BrevlyPrintAgent` — first acquirer wins; D-08 |
| SmartScreen / social engineering (unsigned binary) | Spoofing | OV Authenticode cert + signtool; reputation warmup period documented (D-14) |
| Autostart HKCU Run path hijacking | Elevation of Privilege | Path registered in HKCU (user scope, no elevation); Velopack installs to `%LocalAppData%` (user-writable, non-system) |

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `tao` as event loop for tray-icon | `winit 0.30` ApplicationHandler | Discovered Phase 1 research | tao 0.35 uses closure API incompatible with egui-winit 0.35 |
| tray-icon 0.21 (CLAUDE.md stack doc) | tray-icon 0.24.1 | Phase 1 upgrade | API is compatible; version bumped |
| EV certs bypass SmartScreen instantly | OV and EV both build reputation via downloads | Microsoft policy change March 2024 | OV cert is sufficient; no rush for expensive EV |
| Static HKCU Run exe path | Velopack root stub path detection | Phase 3 new finding | Auto-launch must register stub, not `current\exe` |

**Deprecated / outdated:**
- `tray-icon 0.21` (CLAUDE.md mentions this): current is 0.24.1 — use 0.24.
- `auto-launch 0.5.x` (CLAUDE.md stack doc): current in use is 0.6.0.
- EV certificate requirement for SmartScreen: no longer needed; OV suffices.

---

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | In-loop window create/destroy (`ActivationWindow` → drop → recreate) is clean with winit 0.30 wgpu surface lifecycle | Pattern 7 | wgpu surface panic on re-create; fallback: exit-relaunch for "Reativar" |
| A2 | `HANDLE` from `CreateMutexW` in `windows` 0.62 does not auto-close on drop (no `Drop` impl) | Pattern 5 | If HANDLE is auto-closed, second instance could spawn; verify by holding in `main()` scope |
| A3 | `vpk` CLI is installed via `dotnet tool install -g vpk` on Windows CI runner (dotnet is pre-installed on `windows-latest`) | Pattern 8 | If dotnet not pre-installed, add `actions/setup-dotnet` step |
| A4 | `muda::Menu::append()` returns a `Result` | Pattern 4 | If it panics on error, use `.unwrap()` in dev, handle in prod |

**If this table is empty:** It is not empty; the above assumptions should be verified during task execution.

---

## Open Questions (RESOLVED)

1. **In-loop ActivationWindow recreation with wgpu**
   - What we know: winit 0.30 supports multiple windows; wgpu surface is tied to the window; drop order matters.
   - What's unclear: whether dropping `ActivationWindow` (which owns its wgpu `Surface` + `Device`) then immediately creating a new one in the same event loop process causes any wgpu device state leaks on Windows DX12.
   - Recommendation: spike this in the first task of Phase 3. If it fails, fall back to exit-relaunch on "Reativar" (D-10 allows this).
   - **RESOLVED (03-02 Plan, Task 1):** Use `Option<ActivationWindow>` — drop the existing instance before creating a new one. The `App.activation_window` field is set to `None` before reconstructing; wgpu surface lifetime is bounded to the struct. If device leaks surface on DX12, the exit-relaunch fallback (D-10) is explicitly permitted.

2. **`vpk` version and `dotnet` on `windows-latest` GHA runner**
   - What we know: `vpk` is a `dotnet` global tool; `windows-latest` runners include dotnet 8/9.
   - What's unclear: whether `dotnet tool install -g vpk` downloads vpk 1.x or an older version.
   - Recommendation: pin the version: `dotnet tool install -g vpk --version 1.*`.
   - **RESOLVED (03-03 Plan, Task 1):** CI step pins `dotnet tool install -g vpk --version "1.*"` — always installs 1.x regardless of toolchain defaults.

3. **`HKCU Run` path format with `auto-launch` 0.6**
   - What we know: `auto-launch` 0.6 on Windows writes `{app_path} {args}` to `SOFTWARE\Microsoft\Windows\CurrentVersion\Run`. The `app_path` comes from `AutoLaunch::new(name, app_path, mode, args)`.
   - What's unclear: whether Phase 2's `auto-launch` call used `current_exe()` or a constructed path, and whether that path was the Velopack stub or the `current\` exe.
   - Recommendation: audit Phase 2's `activation_state.rs` save flow for the exact `app_path` argument. If it used `current_exe()`, update to use Pattern 8's stub detection.
   - **RESOLVED (03-02 Plan, Task 3):** `activation_state.rs` save flow is fixed — detect `Update.exe` sibling in parent dir of `current_exe()`; if found, register grandparent path (the Velopack root stub) instead. Comment `// RUN-03` marks the fix.

---

## Sources

### Primary (HIGH confidence — verified from installed crate source)
- `tray-icon 0.24.1` `/home/zephyr/.cargo/registry/src/.../tray-icon-0.24.1/src/lib.rs` — `TrayIconEvent::set_event_handler`, `TrayIconBuilder`, `TrayIcon::set_icon()` API; timing note for `StartCause::Init`
- `tray-icon 0.24.1` `src/icon.rs` — `Icon::from_rgba(Vec<u8>, u32, u32) -> Result<Self, BadIcon>`
- `muda 0.19.3` `src/items/normal.rs` — `MenuItem::new(text, enabled, accel)`, `set_text()`, `set_enabled()`
- `muda 0.19.3` `src/lib.rs` — `Menu::new()`, `Menu::append()`, `MenuEvent`
- `windows 0.62.2` `src/Windows/Win32/System/Threading/mod.rs` — `CreateMutexW` signature
- `windows 0.62.2` `src/Windows/Win32/Foundation/mod.rs` — `GetLastError()`, `ERROR_ALREADY_EXISTS = WIN32_ERROR(183u32)`
- `windows 0.62.2` `src/Windows/Win32/UI/WindowsAndMessaging/mod.rs` — `MessageBoxW`, `MB_YESNO`, `IDYES`, `MB_ICONQUESTION`
- `windows 0.62.2` `Cargo.toml` lines 650, 707 — `Win32_System_Threading`, `Win32_UI_WindowsAndMessaging` feature flag names
- `velopack 1.2.0` `src/locator.rs` — `CurrentBinaryDir: root_dir.join("current")`, `create_config_from_root_dir` pattern
- `auto-launch 0.6.0` `src/windows.rs` — HKCU Run registration with `{app_path} {args}` pattern
- `src/printer/spooler.rs` (existing Phase 2 code) — WR-06 PCWSTR pattern confirmed: `.encode_utf16().chain(std::iter::once(0)).collect()`

### Secondary (MEDIUM confidence — official documentation)
- [Velopack Windows Overview](https://docs.velopack.io/packaging/operating-systems/windows) — root stub path confirmed: "root-level YourApp.exe is a small execution stub… shortcuts/launchers can point at a stable path that survives updates"
- [Velopack vpk Windows CLI](https://docs.velopack.io/reference/cli/content/vpk-windows) — `--packId`, `--packVersion`, `--packDir`, `--mainExe`, `--signParams` parameter names confirmed
- [Velopack Rust Getting Started](https://docs.velopack.io/getting-started/rust) — `vpk pack -u MyAppUniqueId -v 1.0.0 -p /target/release -e myexename.exe` command confirmed
- [Microsoft Learn — New-SelfSignedCertificate](https://learn.microsoft.com/en-us/powershell/module/pki/new-selfsignedcertificate) — self-signed cert for test signing

### Tertiary (LOW confidence — training knowledge / single source)
- SmartScreen reputation policy change March 2024 (OV = EV for reputation purposes) — [cited in CLAUDE.md; CITED: Microsoft Learn SmartScreen reputation docs referenced in CLAUDE.md]
- `Global\` vs `Local\` mutex namespace semantics — standard Win32 knowledge; LOW confidence on edge cases (e.g., UAC-elevated second process)

---

## Metadata

**Confidence breakdown:**
- Headless tray + winit integration: HIGH — API verified from tray-icon 0.24.1 source; matches Phase 1 research
- Icon RGBA construction: HIGH — `Icon::from_rgba` signature confirmed from icon.rs
- Single-instance mutex: HIGH — `CreateMutexW` + `GetLastError` + `ERROR_ALREADY_EXISTS` all verified from windows 0.62.2 source
- MessageBoxW dialogs: HIGH — signature and constants verified from windows 0.62.2 source
- Velopack directory structure + stub path: HIGH — confirmed from official docs + locator.rs source
- vpk CLI command syntax: HIGH — confirmed from official CLI reference
- Signtool CI step: MEDIUM — pattern is standard; exact GitHub Actions runner availability of dotnet/signtool assumed
- Self-signed cert PowerShell flow: MEDIUM — well-known pattern; exact syntax may vary by Windows SDK version

**Research date:** 2026-07-16
**Valid until:** 2026-09-16 (stable APIs; Velopack directory structure unlikely to change)
