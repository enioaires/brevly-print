//! Pusher Channels WebSocket reconnect loop, dedup fence, and dev shim.
//!
//! Provides [`run_pusher_loop`]: a never-returning async function that maintains
//! a Pusher private-channel connection, handles subscribe + ping/pong zombie
//! detection, persists arriving events with `INSERT OR IGNORE` (C3 dedup fence),
//! and forwards new events to the Phase 5 print worker via `mpsc`.
//!
//! Cross-platform — no `#[cfg(windows)]` needed here. The spawn site in
//! `main.rs` Runtime mode is already Windows-only.
//!
//! # Architecture
//!
//! ```text
//! run_pusher_loop
//!   ├── [#cfg(debug_assertions)] try_fake_pusher_event  →  shim path (real WS bypassed)
//!   └── reconnect loop
//!         1. send_health(Reconnecting)
//!         2. connect_async → pusher:connection_established → extract socket_id
//!         3. pusher_auth() [fresh every reconnect — EVT-02]
//!         4. pusher:subscribe → pusher_internal:subscription_succeeded → send_health(Connected)
//!         5. inner event loop (select! on ws.next() + 30s ping timer)
//!            ├── pusher:pong        → clear awaiting_pong
//!            ├── print:job          → INSERT OR IGNORE → mpsc send (skip on dup)
//!            ├── pusher:error       → break inner loop
//!            └── Close / None / Err → break inner loop
//!         6. backoff_delay(attempt) → attempt += 1 → goto 1
//! ```

use std::path::PathBuf;

use anyhow::Context as _;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::{
    health_state::HealthState,
    noren_client::pusher_auth,
    pusher::{
        backoff::backoff_delay,
        protocol::{extract_socket_id, parse_print_job, PrintEvent, PusherConfig, PusherEnvelope},
    },
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse a raw WebSocket text frame into a [`PusherEnvelope`].
fn parse_envelope(text: &str) -> anyhow::Result<PusherEnvelope> {
    serde_json::from_str(text).context("Failed to parse Pusher envelope")
}

/// `INSERT OR IGNORE INTO printed_jobs` for the C3 dedup fence (D-03).
///
/// Returns `Ok(true)` if the row was newly inserted (new event), or `Ok(false)`
/// if the `job_id` was already present (Pusher re-delivery — skip mpsc send).
///
/// Uses parameterised SQL to prevent injection (T-04-07).
fn insert_print_job(
    conn: &rusqlite::Connection,
    job_id: &str,
    job_type: &str,
) -> rusqlite::Result<bool> {
    let changes = conn.execute(
        "INSERT OR IGNORE INTO printed_jobs (job_id, job_type, status, received_at)
         VALUES (?1, ?2, 'pending', datetime('now'))",
        rusqlite::params![job_id, job_type],
    )?;
    Ok(changes > 0)
}

/// Dev shim (D-04) — compiles out entirely in `--release`.
///
/// When `BREVLY_FAKE_PUSHER_EVENT=<jobId>:<type>` is set, spawns a task that
/// sends a synthetic [`PrintEvent`] after a 1-second delay and returns `true`
/// (caller should skip the real WebSocket connection).
///
/// Returns `false` if the env var is absent or malformed (no panic).
#[cfg(debug_assertions)]
fn try_fake_pusher_event(tx: &mpsc::Sender<PrintEvent>, override_val: Option<&str>) -> bool {
    // `override_val` is `Some(value)` in tests (avoids unsafe env mutation);
    // at the real call site, pass `None` to read from the environment variable.
    let raw = match override_val
        .map(|s| s.to_string())
        .or_else(|| std::env::var("BREVLY_FAKE_PUSHER_EVENT").ok())
    {
        Some(v) => v,
        None => return false,
    };

    let (job_id, job_type) = match raw.split_once(':') {
        Some((id, t)) => (id.to_string(), t.to_string()),
        None => {
            eprintln!(
                "[brevly-print] BREVLY_FAKE_PUSHER_EVENT: invalid format '{}' \
                 (expected <jobId>:<type>)",
                raw
            );
            return false;
        }
    };

    let tx = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = tx.send(PrintEvent { job_id, job_type }).await;
    });

    true
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Run the Pusher reconnect loop for the process lifetime (never returns).
///
/// Opens a dedicated SQLite connection for the Pusher task (WAL mode — Pitfall 5)
/// so concurrent writes from the main App connection are safe.
///
/// Health transitions are communicated to the tray via `send_health` (a closure
/// that calls `proxy.send_event(UserEvent::HealthChanged(_))` — C2 constraint:
/// the Pusher task never touches tray APIs directly — Pitfall 4).
///
/// # Arguments
///
/// * `config`       — Pusher credentials (key, cluster, tenant_id, auth_url).
/// * `agent_token`  — Bearer token for the Noren Pusher auth POST (never logged).
/// * `tx`           — mpsc sender to the Phase 5 print worker.
/// * `send_health`  — closure that drives the tray health indicator.
/// * `db_path`      — path to `state.db`; a second connection is opened here.
/// * `http`         — shared reqwest client for the auth POST.
pub async fn run_pusher_loop(
    config: PusherConfig,
    agent_token: String,
    tx: mpsc::Sender<PrintEvent>,
    send_health: impl Fn(HealthState) + Send + 'static,
    db_path: PathBuf,
    http: reqwest::Client,
) {
    // D-04 dev shim: if the fake-event env var is set, skip the real WS connection.
    #[cfg(debug_assertions)]
    if try_fake_pusher_event(&tx, None) {
        send_health(HealthState::Connected);
        std::future::pending::<()>().await;
        // unreachable but required for type inference
        return;
    }

    // Open a SECOND SQLite connection for this task (Pitfall 5 — rusqlite::Connection
    // is not Send; App.conn lives on the event-loop thread).
    let pusher_conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[brevly-print] Pusher task: failed to open SQLite connection: {e:#}");
            return;
        }
    };
    // Enable WAL mode so concurrent writers from the main App connection are safe.
    if let Err(e) = pusher_conn.pragma_update(None, "journal_mode", "WAL") {
        eprintln!("[brevly-print] Pusher task: failed to set WAL mode: {e:#}");
        return;
    }

    let channel = format!("private-tenant-{}-print", config.tenant_id);
    let ws_url = format!(
        "wss://ws-{cluster}.pusher.com/app/{key}?protocol=7&client=brevly-print&version=0.1.0",
        cluster = config.cluster,
        key = config.key,
    );

    let mut attempt = 0u32;

    loop {
        // Step 1: signal reconnecting (yellow tray — D-07)
        send_health(HealthState::Reconnecting);

        // Step 2: WebSocket connect
        let (mut ws, _) = match connect_async(&ws_url).await {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("[brevly-print] Pusher: WS connect failed (attempt {attempt}): {e:#}");
                tokio::time::sleep(backoff_delay(attempt)).await;
                attempt += 1;
                continue;
            }
        };

        // Step 3: expect pusher:connection_established, extract socket_id
        let socket_id = match ws.next().await {
            Some(Ok(Message::Text(text))) => {
                let env = match parse_envelope(text.as_str()) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("[brevly-print] Pusher: failed to parse first frame: {e:#}");
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                };
                if env.event != "pusher:connection_established" {
                    eprintln!(
                        "[brevly-print] Pusher: expected connection_established, got '{}' \
                         (data: {})",
                        env.event, env.data
                    );
                    tokio::time::sleep(backoff_delay(attempt)).await;
                    attempt += 1;
                    continue;
                }
                match extract_socket_id(&env) {
                    Ok(id) => id,
                    Err(e) => {
                        eprintln!("[brevly-print] Pusher: failed to extract socket_id: {e:#}");
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                }
            }
            other => {
                eprintln!("[brevly-print] Pusher: unexpected first message: {other:?}");
                tokio::time::sleep(backoff_delay(attempt)).await;
                attempt += 1;
                continue;
            }
        };

        // Step 4: fresh Pusher channel auth (EVT-02 — never cache across reconnects)
        let auth = match pusher_auth(&http, &config.auth_url, &agent_token, &channel, &socket_id).await {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[brevly-print] Pusher: auth failed (attempt {attempt}): {e:#}");
                tokio::time::sleep(backoff_delay(attempt)).await;
                attempt += 1;
                continue;
            }
        };

        // Step 5: send subscribe message
        let subscribe_msg = serde_json::json!({
            "event": "pusher:subscribe",
            "data": { "channel": channel, "auth": auth }
        });
        if let Err(e) = ws.send(Message::Text(subscribe_msg.to_string().into())).await {
            eprintln!("[brevly-print] Pusher: subscribe send failed: {e:#}");
            tokio::time::sleep(backoff_delay(attempt)).await;
            attempt += 1;
            continue;
        }

        // Step 6: wait for subscription_succeeded before entering the dispatch loop
        // (Pitfall 8 — must not dispatch print:job events before subscription is confirmed)
        let subscribed = loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    match parse_envelope(text.as_str()) {
                        Ok(env) if env.event == "pusher_internal:subscription_succeeded" => {
                            break true;
                        }
                        Ok(env) if env.event == "pusher:error" => {
                            eprintln!(
                                "[brevly-print] Pusher: error before subscription_succeeded — reconnecting. Details: {}",
                                env.data
                            );
                            break false;
                        }
                        Ok(_) => {
                            // ignore other messages while waiting for subscription
                        }
                        Err(e) => {
                            eprintln!("[brevly-print] Pusher: envelope parse error during subscribe: {e:#}");
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => break false,
                Some(Err(e)) => {
                    eprintln!("[brevly-print] Pusher: WS error during subscribe: {e:#}");
                    break false;
                }
                _ => {}
            }
        };

        if !subscribed {
            tokio::time::sleep(backoff_delay(attempt)).await;
            attempt += 1;
            continue;
        }

        // Successfully subscribed — transition tray to Connected (green)
        send_health(HealthState::Connected);
        // Reset attempt counter on successful connect
        attempt = 0;

        // Step 7: inner event loop — ping/pong zombie detection + event dispatch
        let mut ping_timer = interval(Duration::from_secs(30));
        // Use Delay so a sleep/wake burst doesn't fire multiple ticks and trigger
        // a spurious zombie-reconnect (WR-04 — MissedTickBehavior::Burst is default).
        ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // First tick fires immediately; burn it so the first real tick is at 30s
        ping_timer.tick().await;
        let mut awaiting_pong = false;

        let disconnected = 'inner: loop {
            tokio::select! {
                msg = ws.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            let env = match parse_envelope(text.as_str()) {
                                Ok(e) => e,
                                Err(err) => {
                                    eprintln!("[brevly-print] Pusher: envelope parse error: {err:#}");
                                    continue;
                                }
                            };

                            match env.event.as_str() {
                                "pusher:pong" => {
                                    awaiting_pong = false;
                                }
                                "pusher_internal:subscription_succeeded" => {
                                    // Already subscribed — can arrive again after reconnect handshake edge case
                                    send_health(HealthState::Connected);
                                }
                                "print:job" => {
                                    // Double-decode data field (Pitfall 3, D-08)
                                    match parse_print_job(&env) {
                                        Ok(event) => {
                                            match insert_print_job(&pusher_conn, &event.job_id, &event.job_type) {
                                                Ok(true) => {
                                                    // New event — send to Phase 5 worker.
                                                    // WR-04: use try_send to avoid blocking inside
                                                    // the select! arm; a blocking .await here would
                                                    // starve the ping_timer arm and disable zombie
                                                    // detection for the duration of any channel backlog.
                                                    match tx.try_send(event) {
                                                        Ok(()) => {}
                                                        Err(tokio::sync::mpsc::error::TrySendError::Full(ev)) => {
                                                            // Channel full — fall back to a spawned task so
                                                            // the ping timer is not blocked while we wait.
                                                            eprintln!(
                                                                "[brevly-print] Pusher: print channel full — \
                                                                 job {} queued via background send",
                                                                ev.job_id
                                                            );
                                                            let tx2 = tx.clone();
                                                            tokio::spawn(async move {
                                                                let _ = tx2.send(ev).await;
                                                            });
                                                        }
                                                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                                            eprintln!(
                                                                "[brevly-print] Pusher: print channel closed — exiting"
                                                            );
                                                            break 'inner true;
                                                        }
                                                    }
                                                }
                                                Ok(false) => {
                                                    // Duplicate (Pusher re-delivery) — skip mpsc send (C3)
                                                    eprintln!(
                                                        "[brevly-print] Pusher: duplicate job_id skipped (C3 fence)"
                                                    );
                                                }
                                                Err(e) => {
                                                    eprintln!("[brevly-print] Pusher: SQLite insert failed: {e:#}");
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("[brevly-print] Pusher: failed to decode print:job: {e:#}");
                                        }
                                    }
                                }
                                "pusher:error" => {
                                    eprintln!(
                                        "[brevly-print] Pusher: received pusher:error — reconnecting. Details: {}",
                                        env.data
                                    );
                                    break 'inner true;
                                }
                                _ => {
                                    // Unknown event — log and ignore (D-08 graceful extension point)
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            eprintln!("[brevly-print] Pusher: WS closed — reconnecting");
                            break 'inner true;
                        }
                        Some(Err(e)) => {
                            eprintln!("[brevly-print] Pusher: WS error: {e:#}");
                            break 'inner true;
                        }
                        _ => {
                            // Protocol-level Ping/Pong frames — tungstenite handles auto-pong
                        }
                    }
                }
                _ = ping_timer.tick() => {
                    if awaiting_pong {
                        // Zombie: missed pong within 30s window (EVT-03, D-05)
                        eprintln!("[brevly-print] Pusher: zombie connection (missed pong) — reconnecting");
                        break 'inner true;
                    }
                    let ping = serde_json::json!({"event": "pusher:ping", "data": {}});
                    if let Err(e) = ws.send(Message::Text(ping.to_string().into())).await {
                        eprintln!("[brevly-print] Pusher: ping send failed: {e:#}");
                        break 'inner true;
                    }
                    awaiting_pong = true;
                }
            }
        };

        // Inner loop ended — prepare for backoff reconnect
        let _ = disconnected; // logged above

        tokio::time::sleep(backoff_delay(attempt)).await;
        attempt += 1;
        // loop back to step 1: send_health(Reconnecting) at top of outer loop
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Create an in-memory SQLite connection with the printed_jobs schema.
    fn make_test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory DB");
        conn.execute_batch(
            "CREATE TABLE printed_jobs (
                job_id      TEXT PRIMARY KEY NOT NULL,
                job_type    TEXT,
                status      TEXT NOT NULL DEFAULT 'pending',
                attempt     INTEGER NOT NULL DEFAULT 0,
                received_at TEXT,
                printed_at  TEXT,
                failed_at   TEXT
            );",
        )
        .expect("create test schema");
        conn
    }

    // ── insert_print_job dedup ──────────────────────────────────────────────

    #[test]
    fn insert_print_job_returns_true_on_first_insert() {
        let conn = make_test_conn();
        let result = insert_print_job(&conn, "job-001", "order");
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        assert!(result.unwrap(), "first insert should return true");
    }

    #[test]
    fn insert_print_job_returns_false_on_duplicate() {
        let conn = make_test_conn();
        // First insert — new row
        let first = insert_print_job(&conn, "job-001", "order").expect("first insert");
        assert!(first, "first insert should return true");

        // Second insert of the same job_id — INSERT OR IGNORE no-ops
        let second = insert_print_job(&conn, "job-001", "order").expect("second insert");
        assert!(!second, "duplicate insert should return false (C3 fence)");
    }

    #[test]
    fn insert_print_job_different_job_ids_both_insert() {
        let conn = make_test_conn();
        assert!(insert_print_job(&conn, "job-001", "order").unwrap());
        assert!(insert_print_job(&conn, "job-002", "dispatch").unwrap());
    }

    #[test]
    fn insert_print_job_writes_pending_status() {
        let conn = make_test_conn();
        insert_print_job(&conn, "job-xyz", "closing").expect("insert");
        let status: String = conn
            .query_row(
                "SELECT status FROM printed_jobs WHERE job_id = ?1",
                rusqlite::params!["job-xyz"],
                |row| row.get(0),
            )
            .expect("query status");
        assert_eq!(status, "pending");
    }

    // ── try_fake_pusher_event ───────────────────────────────────────────────

    #[cfg(debug_assertions)]
    #[test]
    fn fake_event_returns_false_when_env_not_set() {
        // Pass None explicitly — no env var read, no unsafe mutation needed.
        let (tx, _rx) = mpsc::channel::<PrintEvent>(8);
        assert!(
            !try_fake_pusher_event(&tx, None),
            "should return false when no override and env var is absent"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn fake_event_returns_false_on_malformed_value() {
        // A value without a colon is malformed — should return false, not panic.
        let (tx, _rx) = mpsc::channel::<PrintEvent>(8);
        let result = try_fake_pusher_event(&tx, Some("no-colon-here"));
        assert!(!result, "malformed value should return false");
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn fake_event_parses_job_id_and_type() {
        let (tx, mut rx) = mpsc::channel::<PrintEvent>(8);
        let activated = try_fake_pusher_event(&tx, Some("abc123:order"));

        assert!(activated, "should return true for valid env var");

        // The shim spawns a 1s delayed send — wait for it
        let event = tokio::time::timeout(
            Duration::from_secs(3),
            rx.recv(),
        )
        .await
        .expect("timed out waiting for fake event")
        .expect("channel closed unexpectedly");

        assert_eq!(event.job_id, "abc123");
        assert_eq!(event.job_type, "order");
    }
}
