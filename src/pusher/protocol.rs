//! Pusher Channels protocol types and parsers.
//!
//! Portable — no `#[cfg(windows)]` gate. Provides the typed domain structs
//! (`PusherEnvelope`, `PrintEvent`, `PusherConfig`) and pure parsing functions
//! (`parse_print_job`, `extract_socket_id`) consumed by the reconnect loop.

use anyhow::Context as _;

// ── Types ────────────────────────────────────────────────────────────────────

/// Outer Pusher WebSocket message envelope.
///
/// The `data` field is a JSON-encoded string for system events and ALL app
/// events (double-decode required — Pitfall 3, D-08).
#[derive(serde::Deserialize, Debug)]
pub struct PusherEnvelope {
    pub event: String,
    /// May be a `Value::String` (all system + app events) or `Value::Object`
    /// (only `pusher:error` and some internal frames — treated as invalid here).
    pub data: serde_json::Value,
    #[serde(default)]
    pub channel: Option<String>,
}

/// Payload inside a `print:job` event's `data` field (after double-decode).
///
/// Noren emits `{ "jobId": "...", "type": "..." }` inside the JSON-encoded
/// `data` string of the `print:job` Pusher event.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct PrintEvent {
    #[serde(rename = "jobId")]
    pub job_id: String,
    #[serde(rename = "type")]
    pub job_type: String,
}

/// Config bundle passed to the Pusher reconnect loop.
///
/// Populated from `ConfigStore` at Runtime-mode startup (D-01).
#[derive(Clone)]
pub struct PusherConfig {
    pub key: String,
    pub cluster: String,
    pub tenant_id: String,
    /// Base URL for the Noren API (used for `pusher_auth()` POST).
    pub auth_url: String,
}

// ── Internal helper ──────────────────────────────────────────────────────────

/// Internal helper for `connection_established` parsing.
#[derive(serde::Deserialize)]
struct ConnectionEstablishedData {
    socket_id: String,
}

// ── Public parsing functions ──────────────────────────────────────────────────

/// Double-decode the `data` field of a `print:job` Pusher envelope into a [`PrintEvent`].
///
/// The Pusher protocol wraps the `data` field as a JSON-encoded string for ALL app events
/// (Pitfall 3 / D-08). The outer envelope must be parsed first; this function then parses
/// the inner JSON string.
///
/// # Errors
///
/// Returns `Err` if:
/// - `env.data` is not a `serde_json::Value::String` (defensive contract)
/// - The inner string cannot be decoded as `{ jobId, type }`
pub fn parse_print_job(env: &PusherEnvelope) -> anyhow::Result<PrintEvent> {
    let data_str = match &env.data {
        serde_json::Value::String(s) => s.as_str(),
        other => {
            return Err(anyhow::anyhow!(
                "parse_print_job: expected JSON string in data field, got: {other}"
            ))
        }
    };
    serde_json::from_str::<PrintEvent>(data_str)
        .context("parse_print_job: failed to decode print job payload (double-decode)")
}

/// Extract the `socket_id` from a `pusher:connection_established` envelope.
///
/// The `data` field of `connection_established` is also JSON-in-JSON (double-encoded).
/// Format: `{"socket_id":"123.456","activity_timeout":120}`.
///
/// # Errors
///
/// Returns `Err` if `env.data` is not a string or if `socket_id` is absent.
pub fn extract_socket_id(env: &PusherEnvelope) -> anyhow::Result<String> {
    let data_str = env
        .data
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("extract_socket_id: connection_established data is not a string"))?;
    let data: ConnectionEstablishedData = serde_json::from_str(data_str)
        .context("extract_socket_id: failed to parse connection_established data")?;
    Ok(data.socket_id)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_envelope_string(event: &str, data_str: &str) -> PusherEnvelope {
        PusherEnvelope {
            event: event.to_string(),
            data: serde_json::Value::String(data_str.to_string()),
            channel: None,
        }
    }

    fn make_envelope_object(event: &str, data_obj: serde_json::Value) -> PusherEnvelope {
        PusherEnvelope {
            event: event.to_string(),
            data: data_obj,
            channel: None,
        }
    }

    // ── parse_print_job ──────────────────────────────────────────────────────

    #[test]
    fn parse_print_job_double_decodes_string_data() {
        // data field is a JSON-encoded STRING containing the actual payload
        let inner = r#"{"jobId":"abc","type":"order"}"#;
        let env = make_envelope_string("print:job", inner);

        let result = parse_print_job(&env).expect("should parse successfully");
        assert_eq!(result.job_id, "abc");
        assert_eq!(result.job_type, "order");
    }

    #[test]
    fn parse_print_job_returns_err_when_data_is_object() {
        // Defensive: if data is already an object (not a string), return Err
        let env = make_envelope_object(
            "print:job",
            serde_json::json!({"jobId": "abc", "type": "order"}),
        );
        assert!(
            parse_print_job(&env).is_err(),
            "expected Err when data is not a JSON string"
        );
    }

    // ── extract_socket_id ────────────────────────────────────────────────────

    #[test]
    fn extract_socket_id_parses_connection_established() {
        let inner = r#"{"socket_id":"123.456","activity_timeout":120}"#;
        let env = make_envelope_string("pusher:connection_established", inner);

        let socket_id = extract_socket_id(&env).expect("should extract socket_id");
        assert_eq!(socket_id, "123.456");
    }
}
