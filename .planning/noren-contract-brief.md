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

## ⛔ ESTA FASE É PURAMENTE ADITIVA — NÃO remover o QZ Tray

O QZ Tray está **imprimindo em produção agora** (cliente-0 Haru; Phase 36 "Printing go-live
hardening" shipada, Phase 37 endurecendo o fluxo). O QZ toca o PDV inteiro: `src/lib/pdv/print.ts`
e afins, `PrinterWizard/PrinterSelector/QzTrayStatusBadge.svelte`, `admin/pdv/+page.svelte`,
`admin/settings/printer/`, `api/qz/cert`.

- **NÃO** remover/alterar o fluxo QZ nesta fase. QZ e Brevly Print **convivem** durante a transição.
- **NÃO** apagar/recriar a página de setup de impressora do Noren — a tela nova de ativação/impressora
  vive **no agente Rust** (Fase 2 do Brevly Print, egui), não no Noren. No Noren não há "página nova"
  a criar; a antiga só é **removida numa fase futura**.
- A **aposentadoria do QZ é uma fase Noren separada e futura**, feita só **depois** do cutover
  validado (Brevly Print instalado em campo, assinado, SmartScreen amadurecido ~2-6 semanas,
  imprimindo confiável). Construir o novo é aditivo/seguro; remover o antigo é destrutivo/arriscado —
  não misturar na mesma fase.

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

## ⚠️ Atenção — conflito de código em andamento
Uma fase de impressão do Noren (`confiabilidade-e-fluxo-de-impressao`, ~Phase 37) estaria mexendo
em `buildDespachoTicket` **agora**. **Estabilize essa fase antes** de iniciar a migração ESC/POS
server-side — senão o mesmo builder é editado em duas frentes. Confirmar com `/gsd:progress` no Noren.

## Como o trabalho flui entre os dois repos
- Cada sessão GSD só conhece o próprio `.planning/`. O agente do Noren **não** vê o roadmap do
  Brevly Print — você é a ponte: leva este contrato pra lá.
- Depois de implementado no Noren, nada precisa voltar pra cá. As fases do Brevly Print já têm a
  linha `Depends on (Noren)`; você destrava cada fase daqui conforme o endpoint fica pronto lá.

---
*Briefing criado 2026-07-15. Fatos sobre o Noren a confirmar in-session.*
