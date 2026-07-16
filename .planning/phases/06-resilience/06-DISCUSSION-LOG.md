# Phase 6: Resilience - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-07-16
**Phase:** 06-resilience
**Areas discussed:** All (delegated — Claude's discretion)

---

## Discussion

| Option | Description | Selected |
|--------|-------------|----------|
| Tray durante retry | Ícone vermelho na primeira falha vs. só após 3 tentativas | |
| Texto do toast | Mensagem genérica vs. específica por tipo de erro | |
| Trigger do pull pendentes | Toda reconexão do Pusher vs. só após outage detectado | |
| Pode decidir tudo | Delega todas as decisões técnicas — Claude cria CONTEXT.md pronto | ✓ |

**User's choice:** Delegação total — mesma postura das fases 1, 3, 4 e 5.
**Notes:** Owner confirmou "pode decidir tudo e já criar o contexto, não entendo muito sobre a parte técnica." Todas as decisões foram exercidas pelo Claude com base na análise da base de código existente, schema já presente (retry_queue, HealthState::Problem), e requisitos do ROADMAP.

---

## Claude's Discretion

Todas as decisões desta fase foram tomadas por Claude:

- **D-01**: Schema migration v2 via table-recreation (único approach válido em SQLite para modificar CHECK constraint).
- **D-02**: status='printing' fence inserido imediatamente ANTES de print_raw() — crash recovery fence obrigatório para RES-04.
- **D-03**: Retry como task separada (run_retry_task) em vez de inline no print_worker — melhor separação de responsabilidades; schema já foi desenhado para isso (retry_queue.escpos_bytes BLOB).
- **D-04**: 4ª conexão SQLite WAL — padrão estabelecido nas fases anteriores; suportado pelo WAL.
- **D-05**: Startup crash recovery: query por status='printing' AND NOT IN retry_queue → re-fetch bytes → INSERT retry_queue com next_retry_at=now.
- **D-06**: Poll loop a cada 5s, LIMIT 10 por tick — simples e adequado para o volume esperado (poucos jobs no queue em operação normal).
- **D-07**: Toast genérico em PT-BR: "Falha ao imprimir após 3 tentativas. Verifique se a impressora está ligada e com papel."
- **D-08**: Pending pull em TODA subscription_succeeded — dedup fence (INSERT OR IGNORE) torna isso idempotente e seguro; simplifica o código vs. detectar "estava desconectado".
- **D-09**: Nomes dos campos da resposta /api/agent/jobs/pending TBD — planner deve confirmar no código do Noren antes de definir struct PendingJob.
- **D-10**: Problem tooltip/label mudado de "Problema de conexão" → "Falha na impressora" — mais preciso para o operador.
- **D-11**: run_print_worker NÃO recebe send_health — responsabilidade de health fica no retry_task exclusivamente.
- **D-12**: INSERT OR IGNORE no retry_queue — idempotente caso o mesmo job apareça duas vezes no mpsc.

## Deferred Ideas

- Heartbeat/observabilidade (OBS-01) — v2, fora do roadmap atual
- Paper-level sensing via DLE EOT — v2, serial only
- Typed retry errors (erro específico por tipo de falha de impressora)
- Retry count configurável (ROADMAP especifica 3× / 30s fixo no MVP)
