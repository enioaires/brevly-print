//! Activation form state: `FlowState` enum + `ActivationFormState` struct.
//!
//! This is the UI model for the one-time activation window. All fields are
//! plain data — no async primitives at the struct level except the optional
//! oneshot receiver used to poll the in-flight Noren HTTP response.

use crate::noren_client::{ActivateError, ActivateResponse};
use crate::printer::{enumerate_printers, PrinterEntry};

// ── Flow state ────────────────────────────────────────────────────────────────

/// The activation flow's current stage.
///
/// Plain enum (no `thiserror`) — this is UI state, not an error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowState {
    /// Initial state: serial and printer selected, ready to activate.
    Idle,
    /// Noren HTTP call is in flight.
    ActivationPending,
    /// Token received from Noren; waiting for test-print or manual save.
    ValidatedAwaitingTestPrint,
    /// Test-print dispatched; waiting for user's "Sim/Não" confirmation.
    AwaitingTestConfirm,
    /// Test-print confirmed (or user chose to skip confirmation).
    ReadyToSave,
    /// Save in progress (synchronous; should be very brief).
    Saving,
}

// ── Form state ────────────────────────────────────────────────────────────────

/// All mutable state for the activation form.
///
/// Held inside `ActivationWindow` and mutated each egui frame.
pub struct ActivationFormState {
    // ── Serial field ──────────────────────────────────────────────────────────
    /// Current value of the serial text edit.
    pub serial_input: String,
    /// Inline error message below the serial field (None = no error shown).
    pub serial_error: Option<String>,

    // ── Printer selector ─────────────────────────────────────────────────────
    /// Combined USB + Serial printer list (populated once on init, refreshable).
    pub printer_list: Vec<PrinterEntry>,
    /// Currently selected printer's `display_name` (None = nothing selected yet).
    pub selected_printer: Option<String>,

    // ── Validated token / config (set on 200 OK from Noren) ──────────────────
    /// The agent bearer token; stored only here until saved to DPAPI.
    pub agent_token: Option<String>,
    pub tenant_id: Option<String>,
    pub enabled_types: Vec<String>,
    pub pusher_key: Option<String>,
    pub pusher_cluster: Option<String>,

    // ── UI flow signals ───────────────────────────────────────────────────────
    /// Current stage of the activation flow.
    pub flow: FlowState,
    /// True while an async task is running (HTTP or print job).
    pub is_busy: bool,
    /// True when the window was opened because of a missing/corrupt credential
    /// (i.e., re-activation path). Shows the reassuring banner.
    pub is_reactivation: bool,
    /// True when Noren returned 409 — show the destructive re-bind confirmation.
    pub show_rebind_confirm: bool,
    /// Result of the test-print confirmation: `Some(true)` = Sim, `Some(false)` = Não.
    pub test_print_confirmed: Option<bool>,
    /// Set to true when the test-print call returned a hardware error.
    pub test_print_failed: bool,
    /// Warn message for autostart registration failure (D-13).
    pub autostart_warn: Option<String>,

    // ── Async channel ─────────────────────────────────────────────────────────
    /// One-shot receiver for the in-flight `noren_client::activate` result.
    /// `None` when no call is in flight.
    pub activate_rx: Option<tokio::sync::oneshot::Receiver<Result<ActivateResponse, ActivateError>>>,
}

impl ActivationFormState {
    /// Create initial state.
    ///
    /// Enumerates installed printers (returns empty list on Linux).
    /// Pre-selects the Windows default printer when one exists (D-06).
    pub fn new(is_reactivation: bool) -> Self {
        let printer_list = enumerate_printers();

        // Pre-select the default printer (D-06): find the entry with is_default==true.
        let selected_printer = printer_list
            .iter()
            .find(|p| p.is_default)
            .map(|p| p.display_name.clone());

        Self {
            serial_input: String::new(),
            serial_error: None,
            printer_list,
            selected_printer,
            agent_token: None,
            tenant_id: None,
            enabled_types: Vec::new(),
            pusher_key: None,
            pusher_cluster: None,
            flow: FlowState::Idle,
            is_busy: false,
            is_reactivation,
            show_rebind_confirm: false,
            test_print_confirmed: None,
            test_print_failed: false,
            autostart_warn: None,
            activate_rx: None,
        }
    }

    /// Re-enumerate printers and refresh the list (used by "Atualizar lista" button).
    ///
    /// Tries to preserve the current selection if the same display name still exists.
    pub fn refresh_printers(&mut self) {
        self.printer_list = enumerate_printers();
        // Re-check whether the currently selected printer is still in the list.
        if let Some(sel) = &self.selected_printer {
            if !self.printer_list.iter().any(|p| &p.display_name == sel) {
                self.selected_printer = None;
            }
        }
        // Auto-select default if nothing is selected yet.
        if self.selected_printer.is_none() {
            self.selected_printer = self
                .printer_list
                .iter()
                .find(|p| p.is_default)
                .map(|p| p.display_name.clone());
        }
    }

    /// Clear serial error and rebind confirm when the user changes the serial field.
    ///
    /// Also invalidates any previously received activation token so a serial edit
    /// cannot persist credentials from a prior activation (stale-token bug CR-04).
    pub fn on_serial_changed(&mut self) {
        self.serial_error = None;
        self.show_rebind_confirm = false;
        // Invalidate prior activation result so a serial edit requires re-activation.
        if self.agent_token.is_some() {
            self.agent_token = None;
            self.tenant_id = None;
            self.pusher_key = None;
            self.pusher_cluster = None;
            self.enabled_types.clear();
            self.flow = FlowState::Idle;
            self.test_print_confirmed = None;
        }
    }
}
