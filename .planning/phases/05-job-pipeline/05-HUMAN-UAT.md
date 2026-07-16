---
status: partial
phase: 05-job-pipeline
source: [05-VERIFICATION.md]
started: 2026-07-16T00:00:00Z
updated: 2026-07-16T00:00:00Z
---

## Current Test

[awaiting human testing]

## Tests

### 1. < 1 Second Latency (PRT-06/SC-1)
expected: From Pusher event received to ESC/POS bytes written to printer — wall-clock < 1 second on Windows with a live thermal printer and real Noren connection
result: [pending]

### 2. Three ticket types print correctly (PRT-02/03/04/SC-2)
expected: comanda (order ticket), cupom (receipt), and fechamento (closing slip) all render legibly with correct layout and QR code when triggered from Noren POS
result: [pending]

### 3. USB spooler and COM serial both work (PRT-05/SC-6)
expected: Both printer connection modes (Windows USB spooler via OpenPrinterW and COM serial port) successfully receive and print raw ESC/POS bytes
result: [pending]

### 4. Job-type string cross-check vs Noren emit code (D-06)
expected: The job type strings used in print_worker.rs enabled_types filter exactly match the strings emitted by Noren's backend Pusher events (requires Noren repo access)
result: [pending]

## Summary

total: 4
passed: 0
issues: 0
pending: 4
skipped: 0
blocked: 0

## Gaps
