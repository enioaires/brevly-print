//! Noren HTTP client — serial activation endpoint.
//!
//! Provides `activate()`: POSTs `{ serial, machineId? }` to `POST /api/agent/activate`
//! and maps the response status to typed `ActivateError` variants.
//!
//! The caller creates a `reqwest::Client` once and passes a shared reference to `activate()`
//! (Pitfall 6 — avoids creating a new connection pool per call).
//!
//! Base URL is resolved by `noren_base_url()` which reads the `NOREN_BASE_URL` compile-time
//! environment variable and falls back to `NOREN_BASE_URL_DEFAULT` (Open Question 2).

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default Noren base URL, used when the `NOREN_BASE_URL` env var is not set at compile time.
///
/// IMPORTANT: must use `https://` for TLS validation via reqwest + rustls (T-02-01).
pub const NOREN_BASE_URL_DEFAULT: &str = "https://app.noren.com.br";

/// Resolve the Noren base URL.
///
/// Reads the `NOREN_BASE_URL` compile-time environment variable (set via `cargo build
/// --config 'env.NOREN_BASE_URL="https://..."'` or a `.cargo/config.toml` build script).
/// Falls back to [`NOREN_BASE_URL_DEFAULT`] when the variable is not set.
pub fn noren_base_url() -> String {
    // option_env! evaluates at compile time; no runtime env lookup.
    match option_env!("NOREN_BASE_URL") {
        Some(url) => url.to_string(),
        None => NOREN_BASE_URL_DEFAULT.to_string(),
    }
}

// ── Request / Response types ────────────────────────────────────────────────

/// Payload POSTed to `POST /api/agent/activate`.
#[derive(Serialize)]
struct ActivateRequest<'a> {
    serial: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    machine_id: Option<&'a str>,
    /// CR-03: when true, tells Noren this is a confirmed migration (re-bind) from another machine.
    /// The server must treat `force_rebind: true` as an authorised takeover of the serial.
    /// Omitted on first-time activation (false) to keep the request compact and backward-compatible.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    force_rebind: bool,
}

/// Successful response from `POST /api/agent/activate`.
///
/// Noren returns camelCase JSON; `#[serde(rename_all = "camelCase")]` maps it to
/// Rust snake_case fields (Pitfall 7).
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ActivateResponse {
    pub agent_token: String,
    pub tenant_id: String,
    pub pusher_key: String,
    pub pusher_cluster: String,
    pub enabled_types: Vec<String>,
}

/// Typed error for the `activate()` call.
#[derive(Error, Debug)]
pub enum ActivateError {
    /// HTTP 403 or 404 — the serial is not recognised by Noren.
    #[error("Serial inválido — verifique o código e tente de novo")]
    InvalidSerial,

    /// HTTP 409 — the serial is already registered to another machine.
    ///
    /// The caller may offer a re-bind dialog (CONTEXT D-02).
    #[error("Serial já ativo em outra máquina")]
    AlreadyActiveOther,

    /// Network or HTTP transport failure (connection error, timeout, unexpected status).
    #[error("Erro de rede: {0}")]
    Transport(#[from] reqwest::Error),
}

// ── Public API ──────────────────────────────────────────────────────────────

/// POST `{ serial, machineId? }` to `{base_url}/api/agent/activate` and return the
/// typed response.
///
/// # Arguments
///
/// * `client` — shared `reqwest::Client` (created once by the caller — Pitfall 6).
/// * `base_url` — base URL, e.g. `"https://app.noren.com.br"` (from `noren_base_url()`).
/// * `serial` — activation serial entered by the restaurant owner.
/// * `machine_id` — optional Windows MachineGuid; omitted when `None`.
///
/// # Status mapping
///
/// | HTTP status | Returns |
/// |-------------|---------|
/// | 200         | `Ok(ActivateResponse)` |
/// | 403 or 404  | `Err(ActivateError::InvalidSerial)` |
/// | 409         | `Err(ActivateError::AlreadyActiveOther)` |
/// | other / network error | `Err(ActivateError::Transport(_))` |
///
/// # Security note
///
/// `activate()` never logs the response body — `agentToken` must not appear in logs
/// (T-02-02). The caller is responsible for storing the token via DPAPI (ACT-06).
/// # Arguments
///
/// * `force_rebind` — when `true`, signals to Noren that the user has confirmed migration of
///   the serial from another machine. Set to `false` on first-time activation. CR-03: without
///   this flag a 409 response causes the UI to loop forever on re-bind confirmation.
pub async fn activate(
    client: &reqwest::Client,
    base_url: &str,
    serial: &str,
    machine_id: Option<&str>,
    force_rebind: bool,
) -> Result<ActivateResponse, ActivateError> {
    let url = format!("{base_url}/api/agent/activate");

    let resp = client
        .post(&url)
        .json(&ActivateRequest { serial, machine_id, force_rebind })
        .send()
        .await
        .map_err(ActivateError::Transport)?;

    match resp.status().as_u16() {
        200 => {
            // Deserialize; map JSON error as Transport (wraps reqwest::Error)
            resp.json::<ActivateResponse>()
                .await
                .map_err(ActivateError::Transport)
        }
        403 | 404 => Err(ActivateError::InvalidSerial),
        409 => Err(ActivateError::AlreadyActiveOther),
        _ => {
            // Convert non-2xx status to a reqwest error via error_for_status
            Err(ActivateError::Transport(
                resp.error_for_status().unwrap_err(),
            ))
        }
    }
}

// ── Pusher channel authentication ────────────────────────────────────────────

/// POST channel auth to `{base_url}/api/agent/pusher/auth` and return the
/// Pusher `auth` string (e.g. `"key123:hmac-abc"`).
///
/// # Arguments
///
/// * `client`      — shared `reqwest::Client` (created once by the caller).
/// * `base_url`    — Noren base URL (from `noren_base_url()`).
/// * `agent_token` — Bearer token from `CredentialStore` (never logged — T-04-01).
/// * `channel`     — Pusher private channel name, e.g. `"private-tenant-t1-print"`.
/// * `socket_id`   — socket ID from the current WebSocket session; must be fresh
///                   on every reconnect — never reuse a cached auth string (EVT-02).
///
/// # POST body
///
/// Sends `application/x-www-form-urlencoded` with fields `channel_name` and `socket_id`.
/// CRITICAL: the field name is `channel_name` (not `channel`) — verified from Noren
/// source `/api/agent/pusher/auth/+server.ts` (Pitfall 2).
///
/// # Status mapping
///
/// | HTTP status | Returns |
/// |-------------|---------|
/// | 200         | `Ok(auth_string)` where `auth_string = body.auth` |
/// | 403         | `Err(...)` — invalid token or channel mismatch |
/// | other / transport | `Err(...)` |
///
/// # Security
///
/// `agent_token` is passed via `.bearer_auth()` and is never formatted into any log
/// or error message (T-04-01 / T-02-02).
pub async fn pusher_auth(
    client: &reqwest::Client,
    base_url: &str,
    agent_token: &str,
    channel: &str,
    socket_id: &str,
) -> anyhow::Result<String> {
    // Local deserialize target — avoids polluting the module namespace.
    #[derive(Deserialize)]
    struct PusherAuthResponse {
        auth: String,
    }

    let url = format!("{base_url}/api/agent/pusher/auth");

    let resp = client
        .post(&url)
        .bearer_auth(agent_token)
        // CRITICAL: Noren reads body.get('channel_name') — NOT 'channel' (Pitfall 2)
        .form(&[("channel_name", channel), ("socket_id", socket_id)])
        .send()
        .await
        .context("pusher_auth: HTTP transport error")?;

    match resp.status().as_u16() {
        200 => {
            let body: PusherAuthResponse = resp
                .json()
                .await
                .context("pusher_auth: response parse error")?;
            Ok(body.auth)
        }
        403 => anyhow::bail!("pusher_auth: 403 — invalid token or channel mismatch"),
        status => anyhow::bail!("pusher_auth: unexpected status {status}"),
    }
}
