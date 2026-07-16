---
status: partial
phase: 02-activation
source: [02-VERIFICATION.md]
started: 2026-07-16T02:36:47Z
updated: 2026-07-16T02:36:47Z
blocked_by:
  - "Windows machine with USB thermal printer (+ optional COM device)"
  - "Noren POST /api/agent/activate live + agent_serials table"
---

## Current Test

[awaiting Windows hardware + live Noren endpoint]

## Tests

### 1. ACT-02 — First-run activation window opens
expected: On a Windows machine with no saved credential, launching the agent opens the "Brevly Print — Ativação" window (440×520, non-resizable) with a serial field and printer dropdown.
result: [pending]

### 2. ACT-04 — Combined printer dropdown
expected: Dropdown lists installed Windows printers labelled "(USB)" and COM ports labelled "(Serial)", with the default printer pre-selected; "Atualizar lista" refreshes.
result: [pending]

### 3. ACT-03 — Serial validation against Noren
expected: With the Noren endpoint live, a valid serial shows a spinner (window stays responsive) then advances; an invalid serial shows inline "Serial inválido…" without closing; 409 shows the re-bind confirm and "Confirmar migração" succeeds (force_rebind) without looping.
result: [pending]

### 4. ACT-05 — Test-print reaches the printer (RAW / C1)
expected: "Imprimir teste" prints a legible coupon on the thermal printer AND the paper cuts (validates pDatatype="RAW"). Hardware failure warns but still allows save.
result: [pending]

### 5. ACT-06 — Save persists credential (DPAPI) + config (SQLite)
expected: After "Salvar ativação": credential.bin exists (DPAPI-encrypted), state.db config has printer_name/tenant_id, window closes and process exits cleanly.
result: [pending]

### 6. ACT-07 — Unreadable credential returns to activation
expected: Deleting/corrupting credential.bin and relaunching reopens the activation window with the reactivation banner; a valid credential relaunch shows NO window.
result: [pending]

### 7. ACT-08 — Autostart registered
expected: After save, Task Manager → Startup shows a "Brevly Print" entry; after reboot the agent starts automatically.
result: [pending]

### 8. ACT-01 — Windows installer (Phase 3 dependency)
expected: A downloadable Windows installer installs the agent as a normal program. NOTE: the installer/distributable is produced in Phase 3 — verify once that phase lands.
result: [pending]

## Summary

total: 8
passed: 0
issues: 0
pending: 8
skipped: 0
blocked: 8

## Gaps
