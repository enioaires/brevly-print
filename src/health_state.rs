//! Health state machine for the tray agent.
//!
//! Portable — no `#[cfg(windows)]` on the enum or string mappings.
//! The `#[cfg(windows)]` block adds the `icon()` accessor used by `tray_runtime.rs`.

/// Tri-color connection state reflected in the tray icon (RUN-02).
///
/// Phase 3 seeds `Connected`. Phase 4 (Pusher) drives `Reconnecting`/`Connected`.
/// Phase 6 (printer failure) drives `Problem`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// Green: Pusher connected, printer reachable. Seeded at startup (D-02).
    Connected,
    /// Yellow: WebSocket handshake in progress or reconnect backoff. Phase 4+.
    Reconnecting,
    /// Red: Printer absent or connection failed. D-03 / Phase 6.
    Problem,
}

impl HealthState {
    /// Tooltip shown on hover over the tray icon.
    pub fn tooltip(&self) -> &'static str {
        match self {
            Self::Connected    => "Brevly Print — Conectado",
            Self::Reconnecting => "Brevly Print — Reconectando…",
            Self::Problem      => "Brevly Print — Problema de conexão",
        }
    }

    /// PT-BR label for the disabled status menu item (D-06).
    pub fn status_label(&self) -> &'static str {
        match self {
            Self::Connected    => "Conectado",
            Self::Reconnecting => "Reconectando…",
            Self::Problem      => "Problema de conexão",
        }
    }
}

#[cfg(windows)]
impl HealthState {
    /// Load the corresponding 16×16 RGBA tray icon.
    ///
    /// Assets are embedded at compile time via `include_bytes!` (D-05).
    /// Each file is exactly 16 × 16 × 4 = 1024 bytes of raw RGBA.
    pub fn icon(&self) -> tray_icon::Icon {
        let (bytes, w, h): (&[u8], u32, u32) = match self {
            Self::Connected    => (include_bytes!("../assets/tray_green.rgba"),  16, 16),
            Self::Reconnecting => (include_bytes!("../assets/tray_yellow.rgba"), 16, 16),
            Self::Problem      => (include_bytes!("../assets/tray_red.rgba"),    16, 16),
        };
        tray_icon::Icon::from_rgba(bytes.to_vec(), w, h)
            .expect("embedded tray RGBA bytes are always valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_states_have_distinct_tooltips() {
        let tooltips: Vec<_> = [
            HealthState::Connected,
            HealthState::Reconnecting,
            HealthState::Problem,
        ]
        .iter()
        .map(|s| s.tooltip())
        .collect();
        // All distinct
        assert_eq!(tooltips.len(), 3);
        assert_ne!(tooltips[0], tooltips[1]);
        assert_ne!(tooltips[1], tooltips[2]);
        assert_ne!(tooltips[0], tooltips[2]);
    }

    #[test]
    fn status_labels_are_non_empty() {
        for state in [HealthState::Connected, HealthState::Reconnecting, HealthState::Problem] {
            assert!(!state.status_label().is_empty());
        }
    }
}
