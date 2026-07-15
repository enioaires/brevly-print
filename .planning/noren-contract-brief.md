# Contrato Noren ↔ Brevly Print — briefing de integração

> Documento de **referência cross-repo**. Vive no repo do Brevly Print, mas descreve o trabalho
> que deve ser feito no **Noren** (`~/repos/brevly/noren`), numa sessão GSD separada.
> Cole o resumo na primeira mensagem da sessão do Noren e peça pra ele planejar como fase(s) nova(s).
>
> **Verificar in-session:** os fatos sobre o Noren abaixo vêm de exploração de código (Explore +
> roadmapper) — confirme com `/gsd:progress` e um olhar no código antes de planejar.

## Contexto

O Brevly Print é um agente Rust nativo (repo separado) que substitui o QZ Tray. Ele é um
**spooler burro**: o **Noren renderiza os bytes ESC/POS no servidor**, emite um evento leve no
Pusher, e o agente busca os bytes por HTTP e imprime. Nenhuma lógica de template vai pro Rust.

## O que o Noren JÁ tem (reaproveitar)

- **Pusher em produção**, com canais privados no padrão `private-tenant-{tenantId}-{sufixo}`
  (ex.: `-kitchen`, `-notifications`) e **auth handler existente** (`/api/pusher/auth`,
  cert em `/api/qz/cert`, assinatura em `/api/qz/sign`). O canal do agente será
  `private-tenant-{tenantId}-print`.
- **Builders ESC/POS** em `src/lib/utils/ticket.ts` (`buildTicket`, `buildDespachoTicket`,
  `buildClosingTicket`), com testes em `ticket.test.ts`. Hoje rodam **no cliente** (browser +
  QZ Tray), encoding **ISO-8859-1**, QR nativo via `GS(k`.
- Transições que já disparam impressão e gravam flags (`kitchen_printed_at`,
  `dispatch_printed_at`): pedido `novo→preparando`, `preparando→pronto` (despacho c/ QR via
  `dispatch_token` de 6 chars), aprovação de fechamento de caixa.
- Stack: SvelteKit ^2.63 / Vercel / Postgres (Neon) + Drizzle ^0.45 / Better Auth.

## O que NÃO existe ainda (construir)

O contrato `/api/agent/*` dedicado ao agente + a renderização ESC/POS rodando **no servidor**.

## Migração completa QZ Tray → Brevly Print (NÃO há cliente em produção)

Decisão do dono (2026-07-15): **ninguém está em produção no QZ ainda.** Logo, **não** manter os dois
caminhos nem compatibilidade — fazer o cutover completo numa fase coesa do Noren que remove o QZ e
constrói o novo contrato de uma vez (evita um estado quebrado no meio: sem QZ e sem agente).

**Remover (fluxo QZ)** — confirmar a lista exata in-session:
- `src/lib/pdv/print.ts` e correlatos (`print-status.ts`, `printer-status.ts`, `print-error.ts`)
- componentes `PrinterWizard.svelte`, `PrinterSelector.svelte`, `QzTrayStatusBadge.svelte`
- página `src/routes/admin/settings/printer/` (setup do QZ)
- endpoint `src/routes/api/qz/cert/+server.ts` (e `sign`) e o tipo `src/lib/types/qz-tray.d.ts`

**Religar (impressão deixa de ser disparada pelo cliente):** nos pontos de UI que hoje chamam
impressão via QZ (`admin/pdv/+page.svelte`, `admin/pdv/novo/+page.svelte`, `admin/caixa/+page.svelte`,
`HistoricoCaixasTab.svelte`), **remover a chamada de print do cliente**. No modelo Brevly Print a
impressão vira um **efeito server-side da transição de status**: o servidor grava o job em
`agent_print_jobs` e emite o evento Pusher `{jobId, type}`. O browser não dispara mais nada.

**Sobrevive (migra pro servidor):** só `src/lib/utils/ticket.ts` (+ `ticket.test.ts`) — deixa de rodar
no cliente e passa a gerar os bytes ESC/POS no servidor pro endpoint `/bytes`, preservando ISO-8859-1.

**Sem página de impressora no Noren depois disso:** a tela de ativação/seleção de impressora vive
**no agente Rust** (Fase 2 do Brevly Print, egui). O Noren só ganha a validação de serial (`activate`)
e, futuramente (v2), um toggle por tipo de impressão no dashboard (PREF-01).

---

## Escopo da fase Noren — contrato `/api/agent/*`

### 1. Renderização ESC/POS server-side  ⏱️ maior lead-time — trava a Fase 5 do agente
Mover a geração dos bytes de `ticket.ts` (cliente) para o servidor. **Preservar exatamente
ISO-8859-1** (Node defaulta UTF-8 → usar `Buffer.from(str, 'latin1')` explícito). Validar
`ã ç é ó ú Ç` em impressão real. Reusar/estender `ticket.test.ts` com asserts byte-a-byte.

### 2. Schema (Drizzle)
- `agent_serials`: `serial` (PK), `tenant_id`, `agent_token_hash`, `activated_at`, `revoked_at?`
- `agent_print_jobs`: `id` (PK), `tenant_id`, `type` (`order|dispatch|closing`), `payload_bytes`
  (ou refs pra rerenderizar), `status` (`pending|acked`), `created_at`, `acked_at?`

### 3. `POST /api/agent/activate`
- Req: `{ serial: string, machineId?: string }`
- Resp 200: `{ agentToken: string, tenantId: string, pusherKey: string, pusherCluster: string,
  enabledTypes: ("order"|"dispatch"|"closing")[] }`
- Valida serial → vincula tenant → emite `agentToken` (opaco ou JWT). **Suporta re-ativação**:
  se o serial já ativo for reativado (perda de credencial DPAPI numa reinstalação do Windows),
  invalida o token antigo e emite novo.
- Erros: 404 serial inválido, 409 serial já ativo em outra máquina (política a definir).

### 4. `POST /api/agent/pusher/auth`
- Req (form Pusher): `socket_id`, `channel_name`. Auth via `agentToken` (header Bearer).
- Resp: `{ auth: "<key>:<hmac_sha256>" }` — espelhar o `/api/pusher/auth` existente.
- **Hard 403** se `channel_name !== private-tenant-{tenantId}-print` onde `tenantId` vem do
  token (não do request). Previne vazamento cross-tenant.

### 5. Emissão de evento Pusher leve
Nas 3 transições de impressão, criar a linha em `agent_print_jobs` e disparar no canal
`private-tenant-{tenantId}-print` um evento **leve**:
- `event: "print:job"`, `data: { jobId: string, type: "order"|"dispatch"|"closing" }`
- **Nada de bytes no Pusher** (limite ~10KB; cupom de fechamento estoura). Bytes vão por HTTP.

### 6. `GET /api/agent/jobs/{jobId}/bytes`
- Auth: `agentToken`. Valida que o job pertence ao tenant do token.
- Resp 200: `{ jobId, type, bytesB64: string }` (ESC/POS em base64, ISO-8859-1 preservado).
- Erros: 404 job expirado/inexistente, 403 tenant diferente.

### 7. `POST /api/agent/jobs/{jobId}/ack`
- Auth: `agentToken`. Marca `status=acked`, `acked_at=now()`.
- **Idempotente:** 200 na primeira vez, **409** em repetição (nunca 5xx). O agente trata 409 como sucesso.

### 8. `GET /api/agent/jobs/pending`
- Auth: `agentToken`. Jobs `status=pending` do tenant, `ORDER BY created_at ASC`, **máx 100**.
- Resp: `{ jobs: { jobId, type }[] }`. O agente chama isto a cada reconexão do Pusher (queda de internet).

### 9. `GET /api/agent/version`
- Resp: `{ latest: "x.y.z", downloadUrl: string, sha256: string, minSupported?: string }`.
- Requer **hospedar o binário** (S3/R2/Cloudflare). O agente valida SHA256 antes de aplicar.

---

## Ordem sugerida (por dependência do lado agente)
1. `agent_serials` + `POST /activate`  → destrava **Fase 2 (Ativação)** do agente
2. `POST /pusher/auth` + emissão `print:job`  → destrava **Fase 4 (Pusher)**
3. **ESC/POS server-side** + `GET /bytes` + `POST /ack`  → destrava **Fase 5 (Pipeline, core value)**
4. `GET /jobs/pending`  → destrava **Fase 6 (Resiliência)**
5. `GET /version` + hosting  → destrava **Fase 7 (Auto-update)**

## ⚠️ Atenção — fase de impressão em andamento no Noren
Há indícios de uma fase de impressão do Noren em execução (`confiabilidade-e-fluxo-de-impressao`,
~Phase 37) mexendo em `buildDespachoTicket`/`ticket.ts` — o mesmo código que esta migração absorve.
Como o cutover é completo, **reconcilie com o agente do Noren** se a nova fase deve **suceder,
reaproveitar ou substituir** essa Phase 37, em vez de rodar em paralelo sobre o mesmo `ticket.ts`.
Confirmar o estado real com `/gsd:progress` no Noren antes de planejar.

## Como o trabalho flui entre os dois repos
- Cada sessão GSD só conhece o próprio `.planning/`. O agente do Noren **não** vê o roadmap do
  Brevly Print — você é a ponte: leva este contrato pra lá.
- Depois de implementado no Noren, nada precisa voltar pra cá. As fases do Brevly Print já têm a
  linha `Depends on (Noren)`; você destrava cada fase daqui conforme o endpoint fica pronto lá.

---
*Briefing criado 2026-07-15. Fatos sobre o Noren a confirmar in-session.*
