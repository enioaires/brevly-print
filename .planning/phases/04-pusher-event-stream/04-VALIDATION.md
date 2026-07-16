---
phase: 04
slug: pusher-event-stream
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-07-16
---

# Phase 04 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | none (uses default Cargo test harness) |
| **Quick run command** | `cargo test --lib` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green + manual smoke (SC-1 tray turns green, SC-2 fake event processed)
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| EVT-01-parse | 01 | 1 | EVT-01 | T-38-05 | `parse_print_job()` double-decodes `data` field | unit | `cargo test --lib -- pusher::protocol::tests` | ❌ Wave 0 | ⬜ pending |
| EVT-01-socket | 01 | 1 | EVT-01 | — | `extract_socket_id()` parses connection_established | unit | `cargo test --lib -- pusher::protocol::tests` | ❌ Wave 0 | ⬜ pending |
| EVT-01-auth | 01 | 1 | EVT-01 | T-02-02 | `pusher_auth()` maps 200/403/transport to typed results | unit | `cargo test --test pusher_auth_test` | ❌ Wave 0 | ⬜ pending |
| EVT-02-no-cache | 02 | 2 | EVT-02 | — | Fresh auth POST on every reconnect — never reuses auth string | unit | `cargo test --test pusher_auth_test` | ❌ Wave 0 | ⬜ pending |
| EVT-03-backoff | 02 | 2 | EVT-03 | — | `backoff_delay(attempt)` never exceeds 60 s cap | unit | `cargo test --lib -- pusher::backoff::tests` | ❌ Wave 0 | ⬜ pending |
| EVT-03-zombie | 02 | 2 | EVT-03 | — | `awaiting_pong = true` on second ping tick triggers reconnect | unit | `cargo test --lib -- pusher::client::tests` | ❌ Wave 0 | ⬜ pending |
| D-03-dedup | 01 | 1 | EVT-01 | — | `INSERT OR IGNORE` returns false on duplicate job_id | unit | `cargo test --lib -- pusher::client::tests` | ❌ Wave 0 | ⬜ pending |
| D-04-shim | 01 | 1 | EVT-01 | — | `BREVLY_FAKE_PUSHER_EVENT` parses `jobId:type` correctly | unit | `cargo test --lib -- pusher::tests` | ❌ Wave 0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/pusher_auth_test.rs` — covers EVT-01, EVT-02 HTTP contract (mock TCP listener pattern from `tests/noren_client_test.rs`)
- [ ] `src/pusher/protocol.rs` inline tests — double-decode, socket_id extraction
- [ ] `src/pusher/backoff.rs` inline tests — cap enforcement, jitter range (assert `delay <= 60_000ms`)
- [ ] `src/pusher/client.rs` inline tests — INSERT OR IGNORE dedup logic (in-memory SQLite)

*All Wave 0 stubs use `#[cfg(test)]` modules or integration test files. No new test infra needed — pattern established by `tests/noren_client_test.rs`.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Tray turns green after subscription_succeeded | EVT-01 (SC-1) | Requires Windows + real/test Pusher app | Run release build, verify tray icon turns green after app starts |
| Zombie detection: tray yellow → green on reconnect | EVT-03 (SC-3) | Requires network interruption simulation | Disable network adapter while connected, wait >30s, re-enable, observe tray transitions |
| SC-2: fake event enqueued < 500ms | EVT-01 (SC-2) | Timing test, E2E through mpsc | Set `BREVLY_FAKE_PUSHER_EVENT=test123:order`, observe Phase 5 receives event |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
