# Contrato Noren ↔ Brevly Print — briefing de integração

> Documento de **referência cross-repo**. Vive no repo do Brevly Print, mas descreve o trabalho
> que deve ser feito no **Noren** (`~/repos/brevly/noren`), numa sessão GSD separada.
> Cole a seção "PROMPT PARA O AGENTE DO NOREN" na primeira mensagem da sessão do Noren e peça
> pra ele planejar como fase(s) nova(s).
>
> **Verificar in-session:** os fatos sobre o Noren abaixo vêm de exploração de código — confirme
> com `/gsd:progress` e um olhar no código antes de planejar.

## Contexto

O Brevly Print é um agente Rust nativo (repo separado) que **substitui completamente** o QZ Tray.
Ele é um **spooler burro**: o **Noren renderiza os bytes ESC/POS no servidor**, emite um evento leve
no Pusher na transição de status, grava o job numa fila server-side, e o agente busca os bytes por
HTTP e imprime, confirmando com ack. O browser **não dispara mais impressão** — impressão vira
efeito server-side da transição. Nenhuma lógica de template vai pro Rust.

## Decisão do dono (2026-07-15): LIMPEZA TOTAL, sem convivência

**Não há cliente em produção (só o próprio dev).** Portanto **remover o QZ Tray por completo, sem
manter os dois caminhos, sem compatibilidade**. Trocar o motor inteiro de uma vez, numa fase única e
coesa (destrutiva + construtiva juntas, pra não ficar num estado sem QZ e sem agente no meio).

A tela de setup de impressora nova vive **no agente Rust** (Fase 2 do Brevly Print, egui: serial +
dropdown de impressora + test-print). **No Noren não existirá mais nenhuma página de impressora.**

## O que o Noren JÁ tem (reaproveitar)

- **Pusher em produção**, canais privados `private-tenant-{tenantId}-{sufixo}` (ex.: `-kitchen`,
  `-notifications`, `-audit`) e **auth handler existente** em `/api/pusher/auth`. O canal do agente
  será `private-tenant-{tenantId}-print`.
- **Builders ESC/POS** em `src/lib/utils/ticket.ts` (`buildTicket`, `buildDespachoTicket`,
  `buildClosingTicket`), com testes em `ticket.test.ts`. Hoje rodam **no cliente** (browser +
  QZ Tray), encoding **ISO-8859-1**, QR nativo via `GS(k`.
- Transições que já disparam impressão e gravam flags (`kitchen_printed_at`, `dispatch_printed_at`):
  pedido confirmado, despacho c/ QR (`dispatch_token` 6 chars), aprovação de fechamento de caixa.
- Stack: SvelteKit ^2.63 / Vercel / Postgres (Neon) + Drizzle ^0.45 / Better Auth.

## Arquivos QZ que SAEM (remover)

- `src/lib/pdv/print.ts` (~695 linhas), `print-status.ts`, `printer-status.ts`, `print-error.ts`
- `src/lib/components/PrinterWizard.svelte`, `PrinterSelector.svelte`, `QzTrayStatusBadge.svelte`
- `src/routes/admin/settings/printer/` (página de setup do QZ, inteira)
- `src/routes/api/qz/cert/+server.ts`, `src/lib/types/qz-tray.d.ts`

## Pontos de UI que hoje DISPARAM impressão pelo cliente (arrancar a chamada)

- `src/routes/admin/pdv/+page.svelte`, `src/routes/admin/pdv/novo/+page.svelte`
- `src/routes/admin/caixa/+page.svelte`, `src/lib/components/HistoricoCaixasTab.svelte`

Nesses pontos, **remover a chamada de impressão do cliente** — a impressão passa a ser efeito
server-side da transição de status (o servidor grava o job em `agent_print_jobs` e emite o evento
Pusher `{jobId, type}`).

## O único código de impressão que SOBREVIVE

- `src/lib/utils/ticket.ts` (+ testes) — deixa de rodar no cliente, passa a rodar **no servidor**
  para gerar os bytes ESC/POS do endpoint `/bytes`, **preservando ISO-8859-1**.

---

## Escopo da fase Noren — contrato `/api/agent/*` + limpeza do QZ

### 1. Renderização ESC/POS server-side  ⏱️ maior lead-time — trava a Fase 5 do agente (core value)
Mover a geração dos bytes de `ticket.ts` (cliente) para o servidor. **Preservar exatamente
ISO-8859-1** (Node defaulta UTF-8 → `Buffer.from(str, 'latin1')` explícito). Validar `ã ç é ó ú Ç`
em impressão real. Reusar/estender `ticket.test.ts` com asserts byte-a-byte.

### 2. Schema (Drizzle)
- `agent_serials`: `serial` (PK), `tenant_id`, `agent_token_hash`, `activated_at`, `revoked_at?`, `machine_id?`
- `agent_print_jobs`: `id` (PK), `tenant_id`, `type` (`order|dispatch|closing`), refs pra rerenderizar
  os bytes, `status` (`pending|acked`), `created_at`, `acked_at?`

### 3. `POST /api/agent/activate`
- Req: `{ serial: string, machineId?: string }`
- Resp 200: `{ agentToken, tenantId, pusherKey, pusherCluster, enabledTypes: ("order"|"dispatch"|"closing")[] }`
- Valida serial → vincula tenant → emite `agentToken`. **Suporta re-ativação:** se o serial já ativo
  for reativado (perda de credencial DPAPI numa reinstalação do Windows), invalida o token antigo e
  emite novo.
- Erros: 404 serial inválido, 409 serial já ativo em outra máquina (política a definir).

### 4. `POST /api/agent/pusher/auth`
- Req (form Pusher): `socket_id`, `channel_name`. Auth via `agentToken` (Bearer).
- Resp: `{ auth: "<key>:<hmac_sha256>" }` — espelhar o `/api/pusher/auth` existente.
- **Hard 403** se `channel_name !== private-tenant-{tenantId}-print`, com `tenantId` vindo do token
  (não do request). Previne vazamento cross-tenant.

### 5. Emissão de evento Pusher leve (substitui o disparo QZ do cliente)
Nas 3 transições de impressão, criar a linha em `agent_print_jobs` e disparar no canal
`private-tenant-{tenantId}-print` um evento **leve**:
- `event: "print:job"`, `data: { jobId, type: "order"|"dispatch"|"closing" }`
- **Nada de bytes no Pusher** (limite ~10KB; fechamento estoura). Bytes vão por HTTP.

### 6. `GET /api/agent/jobs/{jobId}/bytes`
- Auth `agentToken`; valida que o job pertence ao tenant do token.
- Resp 200: `{ jobId, type, bytesB64 }` (ESC/POS base64, ISO-8859-1 preservado).
- Erros: 404 expirado/inexistente, 403 tenant diferente.

### 7. `POST /api/agent/jobs/{jobId}/ack`
- Auth `agentToken`; marca `status=acked`, `acked_at=now()`.
- **Idempotente:** 200 na primeira vez, **409** em repetição (nunca 5xx). O agente trata 409 como sucesso.

### 8. `GET /api/agent/jobs/pending`
- Auth `agentToken`; jobs `status=pending` do tenant, `ORDER BY created_at ASC`, **máx 100**.
- Resp: `{ jobs: { jobId, type }[] }`. O agente chama a cada reconexão do Pusher (queda de internet).

### 9. `GET /api/agent/version`
- Resp: `{ latest, downloadUrl, sha256, minSupported? }`.
- Requer **hospedar o binário** (S3/R2/Cloudflare). O agente valida SHA256 antes de aplicar.

### 10. Remoção do QZ Tray (ver listas acima)
Deletar os arquivos QZ e arrancar as chamadas de impressão dos 4 pontos de UI. Sem compatibilidade,
sem convivência. Único sobrevivente: `ticket.ts` (migrado pro servidor).

---

## PROMPT PARA O AGENTE DO NOREN (colar na sessão de `~/repos/brevly/noren`)

Preciso migrar o Noren do QZ Tray para um novo agente de impressão nativo (Brevly Print, app Rust
separado). Modelo novo: o Noren renderiza os bytes ESC/POS no servidor, emite um evento leve no
Pusher na transição de status, grava o job numa fila server-side, e o agente busca os bytes por HTTP
e imprime, confirmando com ack. O browser não dispara mais impressão. Quero planejar isso como fase
nova do GSD (v3.0).

Contexto já existente no Noren (reaproveitar):
- Pusher em produção, canais privados `private-tenant-{tenantId}-{sufixo}` e auth em `/api/pusher/auth`.
  O canal do agente será `private-tenant-{tenantId}-print`.
- Builders ESC/POS em `src/lib/utils/ticket.ts` (`buildTicket`, `buildDespachoTicket`,
  `buildClosingTicket`) com testes em `ticket.test.ts`. Hoje rodam no cliente (browser + QZ Tray),
  encoding ISO-8859-1, QR via `GS(k`.

Escopo da fase:
1. Migrar a renderização ESC/POS de `ticket.ts` para rodar NO SERVIDOR, preservando EXATAMENTE
   ISO-8859-1 (Node defaulta UTF-8 → `Buffer.from(str,'latin1')`; validar `ã ç é ó ú Ç` em impressão
   real). Reusar/estender `ticket.test.ts`. Item de maior prazo.
2. Tabelas novas: `agent_serials` (serial → tenant) e `agent_print_jobs` (fila de jobs, status, ack).
3. `POST /api/agent/activate` — valida serial, vincula tenant, emite `agentToken`, suporta re-ativação
   (perda de credencial DPAPI numa reinstalação: invalida token antigo, emite novo).
4. `POST /api/agent/pusher/auth` — auth HMAC do canal do agente, HARD 403 se `channel_name` não bater
   com o `tenantId` do agentToken. Espelhar o `/api/pusher/auth` existente.
5. Emitir evento Pusher LEVE `{jobId, type}` (`event: print:job`) no canal `private-tenant-{tenantId}-print`
   nas 3 transições (pedido confirmado, despacho c/ QR, aprovação de fechamento), criando a linha em
   `agent_print_jobs`. Nada de bytes no Pusher.
6. `GET /api/agent/jobs/{jobId}/bytes` — bytes ESC/POS (base64) do job, autenticado, valida tenant.
7. `POST /api/agent/jobs/{jobId}/ack` — marca impresso; IDEMPOTENTE (409 no repeat, nunca 5xx).
8. `GET /api/agent/jobs/pending` — não-ackados do tenant, `created_at ASC`, máx 100 (pull offline).
9. `GET /api/agent/version` — última versão + downloadUrl + SHA256. Inclui hospedar o binário.

Esta fase substitui o QZ Tray COMPLETAMENTE — NÃO há cliente em produção, então NÃO manter
compatibilidade nem os dois caminhos. Remover todo o fluxo QZ Tray: `src/lib/pdv/print.ts`,
`print-status.ts`, `printer-status.ts`, `print-error.ts`; os componentes `PrinterWizard.svelte`,
`PrinterSelector.svelte`, `QzTrayStatusBadge.svelte`; a página `src/routes/admin/settings/printer/`;
o endpoint `src/routes/api/qz/cert/+server.ts` e `src/lib/types/qz-tray.d.ts`. Nos pontos de UI que
hoje disparam impressão pelo cliente (`admin/pdv/+page.svelte`, `admin/pdv/novo/+page.svelte`,
`admin/caixa/+page.svelte`, `HistoricoCaixasTab.svelte`), REMOVER a chamada de impressão do cliente —
a impressão passa a ser efeito server-side da transição de status. O único código de impressão que
SOBREVIVE é `src/lib/utils/ticket.ts` (+ testes), migrado pro servidor. Não haverá mais nenhuma
página de impressora no Noren.

Ordem sugerida: tabelas + `activate` primeiro; depois `pusher/auth` + emissão `print:job` nas
transições (junto com arrancar o QZ dos pontos de UI); depois migração ESC/POS + `bytes` + `ack`
(o core); depois `pending`; por último `version`. A Phase 37 atual
(`confiabilidade-e-fluxo-de-impressao`) mexe em `buildDespachoTicket` — esta migração absorve/substitui
esse trabalho; me diga se faz mais sentido esta fase suceder ou reaproveitar a Phase 37.

Analise o estado atual e me proponha como encaixar isso como fase(s) nova(s) do roadmap do Noren.

---

## Ordem sugerida (por dependência do lado agente)
1. `agent_serials` + `POST /activate`  → destrava **Fase 2 (Ativação)** do agente
2. `POST /pusher/auth` + emissão `print:job` (+ arrancar QZ da UI)  → destrava **Fase 4 (Pusher)**
3. **ESC/POS server-side** + `GET /bytes` + `POST /ack`  → destrava **Fase 5 (Pipeline, core value)**
4. `GET /jobs/pending`  → destrava **Fase 6 (Resiliência)**
5. `GET /version` + hosting  → destrava **Fase 7 (Auto-update)**

## ⚠️ Conflito de código em andamento
A Phase 37 do Noren (`confiabilidade-e-fluxo-de-impressao`) mexe em `buildDespachoTicket` **agora**.
Como a limpeza remove o QZ e migra `ticket.ts` pro servidor, esta fase **absorve/substitui** o
trabalho da 37 no mesmo builder. Confirmar com `/gsd:progress` se a nova fase sucede ou reaproveita
a 37, pra não editar o mesmo código em duas frentes.

## Como o trabalho flui entre os dois repos
- Cada sessão GSD só conhece o próprio `.planning/`. O agente do Noren **não** vê o roadmap do
  Brevly Print — você é a ponte: leva este contrato pra lá.
- Depois de implementado no Noren, nada precisa voltar pra cá. As fases do Brevly Print já têm a
  linha `Depends on (Noren)`; você destrava cada fase daqui conforme o endpoint fica pronto lá.

---
*Briefing criado 2026-07-15. Decisão de limpeza total do QZ (sem convivência) confirmada pelo dono.
Fatos sobre o Noren a confirmar in-session.*
