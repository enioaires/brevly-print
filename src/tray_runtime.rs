#![cfg(windows)]
//! Windows tray icon runtime — creation, menu, and health-state updates.
//!
//! **Windows-only.** Compiled only when `cfg(windows)`.
//!
//! Encapsulates all `tray-icon` + `muda` interaction: creates the `TrayIcon` in
//! `new_events(StartCause::Init)`, builds the right-click menu, and applies
//! `HealthState` changes by swapping icon + tooltip + status label.
//!
//! CRITICAL: `TrayRuntime::new()` must be called ONLY from `ApplicationHandler::new_events()`
//! when `cause == StartCause::Init` — the Win32 message pump must be running first.

use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, IDYES,
    MESSAGEBOX_RESULT,
};
use windows::core::PCWSTR;

use tray_icon::{TrayIcon, TrayIconBuilder};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

use crate::health_state::HealthState;

/// Holds the live `TrayIcon` and menu item handles.
///
/// Constructed once in `new_events(StartCause::Init)`; held in `App` for process lifetime.
pub struct TrayRuntime {
    tray: TrayIcon,
    pub menu_items: TrayMenuItems,
}

/// Handles for the four right-click menu items (D-06).
pub struct TrayMenuItems {
    pub status:   MenuItem,   // disabled status line
    pub reativar: MenuItem,
    pub sobre:    MenuItem,
    pub sair:     MenuItem,
}

impl TrayRuntime {
    /// Create the tray icon and right-click menu.
    ///
    /// Must be called from `ApplicationHandler::new_events(StartCause::Init)`.
    pub fn new(health: HealthState) -> anyhow::Result<Self> {
        let (menu, menu_items) = build_tray_menu(health);
        let tray = TrayIconBuilder::new()
            .with_icon(health.icon())
            .with_tooltip(health.tooltip())
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false) // D-07: left-click is no-op
            .build()
            .map_err(|e| anyhow::anyhow!("TrayIconBuilder::build failed: {e}"))?;
        Ok(Self { tray, menu_items })
    }

    /// Swap icon, tooltip, and status label to reflect a new health state.
    ///
    /// Called from `App::user_event(UserEvent::HealthChanged(_))` on the event-loop thread.
    pub fn apply_health(&self, health: HealthState) {
        let _ = self.tray.set_icon(Some(health.icon()));
        let _ = self.tray.set_tooltip(Some(health.tooltip()));
        self.menu_items.status.set_text(health.status_label());
    }

    /// Update the status-line text and tooltip to the "update ready" message (D-04).
    ///
    /// Called from `App::user_event(UserEvent::UpdateStaged)` on the event-loop thread.
    ///
    /// IMPORTANT: does NOT call `set_icon` — the tray icon color is reserved for
    /// connection health state (Phase 3 D-01/D-02). "Update ready" is orthogonal and
    /// lives only in the status-line text + the one-shot toast (D-04).
    pub fn set_update_status(&self) {
        self.menu_items.status.set_text(
            "Atualização pronta — será aplicada ao reiniciar"
        );
        let _ = self.tray.set_tooltip(Some(
            "Brevly Print — Atualização pronta"
        ));
    }

    /// Expose tray menu item IDs for menu event dispatch in `App`.
    pub fn menu_items(&self) -> &TrayMenuItems {
        &self.menu_items
    }
}

/// Build a null-terminated UTF-16 wide string for Win32 PCWSTR parameters.
///
/// WR-06: established pattern from src/printer/spooler.rs — use `.chain(std::iter::once(0))`.
fn to_wstr(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Show the "Sobre" info dialog (D-06).
pub fn show_about_dialog() {
    let version = env!("CARGO_PKG_VERSION");
    let text    = to_wstr(&format!("Brevly Print v{}\n\nAgente de impressão para Noren.", version));
    let caption = to_wstr("Sobre o Brevly Print");
    // SAFETY: Win32 FFI — pointer and length values are correctly derived from owned
    // Vec<u16> references that outlive the unsafe block. NULL hwnd is valid for MessageBoxW.
    unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

/// Show the "Sair" confirmation dialog (D-06). Returns true if user confirmed quit.
pub fn confirm_quit_dialog() -> bool {
    let text = to_wstr(
        "Fechar o Brevly Print?\nAs impressões vão parar enquanto o programa estiver fechado."
    );
    let caption = to_wstr("Brevly Print — Sair");
    // SAFETY: Win32 FFI — pointer and length values are correctly derived from owned
    // Vec<u16> references that outlive the unsafe block. NULL hwnd is valid for MessageBoxW.
    let result: MESSAGEBOX_RESULT = unsafe {
        MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(caption.as_ptr()), MB_YESNO | MB_ICONQUESTION)
    };
    result == IDYES
}

fn build_tray_menu(health: HealthState) -> (Menu, TrayMenuItems) {
    let status   = MenuItem::new(health.status_label(), false, None); // false = disabled
    let reativar = MenuItem::new("Reativar impressora/licença", true, None);
    let sobre    = MenuItem::new("Sobre", true, None);
    let sair     = MenuItem::new("Sair", true, None);

    let menu = Menu::new();
    let _ = menu.append(&status);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&reativar);
    let _ = menu.append(&sobre);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&sair);

    (menu, TrayMenuItems { status, reativar, sobre, sair })
}

#[cfg(test)]
mod tests {
    use tray_icon::menu::MenuItem;

    #[test]
    fn menu_items_have_distinct_ids() {
        let item1 = MenuItem::new("Status",   false, None);
        let item2 = MenuItem::new("Reativar", true,  None);
        let item3 = MenuItem::new("Sobre",    true,  None);
        let item4 = MenuItem::new("Sair",     true,  None);

        let ids = [item1.id(), item2.id(), item3.id(), item4.id()];

        // All IDs must be distinct
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[0], ids[2]);
        assert_ne!(ids[0], ids[3]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[1], ids[3]);
        assert_ne!(ids[2], ids[3]);
    }
}
