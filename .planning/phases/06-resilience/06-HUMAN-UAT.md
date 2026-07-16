---
status: partial
phase: 06-resilience
source: [06-VERIFICATION.md]
started: 2026-07-16T19:45:00Z
updated: 2026-07-16T19:45:00Z
---

## Current Test

[awaiting human testing on a Windows machine]

## Tests

### 1. Windows toast appears after 3 failed retries (RES-02)
expected: With a mock/offline printer that always fails, after the print worker's initial failure + 3 retry-task attempts at 30s intervals (~90s), a Windows toast notification appears with the title "Brevly Print — Falha na impressão" and plain-language body text. Code path verified on Linux (send_health(Problem) + show_print_failure_toast() present in the exhaustion arm at retry_task.rs), but toast rendering is #[cfg(windows)] and cannot run on the Linux CI host.
result: [pending]

### 2. Tray icon turns red after retry exhaustion (RES-02)
expected: After the same 3-retry exhaustion above, the system-tray icon changes to the red "Problem" state with tooltip "Brevly Print — Falha na impressora". Health-state transition logic verified on Linux; the tray WM rendering requires Windows.
result: [pending]

### 3. Crash recovery after a real power-cycle / process kill (RES-04)
expected: Kill the agent process mid-print (while a job is at status='printing'). On restart, recover_orphans() runs before the print worker spawns, re-fetches the ESC/POS bytes, re-queues the orphaned job, and it prints exactly once (SQLite dedup prevents a double-print). Logic + SQL unit-tested on Linux; a real crash/restart cycle on Windows confirms the end-to-end behavior.
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
