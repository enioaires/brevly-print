# Brevly Print

## What This Is

Brevly Print é um agente nativo de impressão para Windows que substitui o QZ Tray no
Noren (SaaS de gestão de restaurantes). O dono do restaurante instala uma vez, ativa com
um serial number e seleciona a impressora térmica — a partir daí o agente roda invisível
na bandeja do sistema e imprime comandas e cupons automaticamente quando eventos chegam
do Noren, sem que o funcionário do caixa precise interagir com nada.

## Core Value

Quando um evento de impressão chega do Noren, a comanda/cupom correto sai na impressora
térmica em menos de 1 segundo, de forma confiável e sem intervenção humana — nenhuma
comanda perdida, mesmo com impressora ou internet fora do ar.

## Requirements

### Validated

(None yet — ship to validate)

### Active

<!-- Hipóteses até shipar e validar. -->

- [ ] Instalador Windows baixável pelo site da Brevly, instala como programa comum
- [ ] Tela de ativação na primeira abertura: input de serial + seleção de impressora + salvar
- [ ] Serial number validado contra o backend do Noren (mapeia serial → tenant/restaurante)
- [ ] Agente inicia automaticamente com o Windows (autostart) e roda invisível
- [ ] Ícone na bandeja do sistema: verde = conectado, vermelho = problema de conexão
- [ ] Recebe eventos de impressão do Noren via Pusher (canal privado por tenant)
- [ ] Busca os bytes ESC/POS do job via HTTP autenticado e confirma entrega (ack)
- [ ] Imprime comanda de pedido (pedido novo confirmado)
- [ ] Imprime comanda do entregador (despacho de entrega, com QR code de despacho)
- [ ] Imprime cupom de fechamento (fechamento de caixa/turno)
- [ ] Cada tipo de impressão pode ser habilitado/desabilitado nas preferências (fonte: Noren)
- [ ] Retenta automaticamente até 3× com intervalo de 30s quando a impressora falha
- [ ] Notificação do Windows + ícone vermelho quando a impressão falha em definitivo
- [ ] Fila server-side: ao reconectar após queda de internet, puxa jobs pendentes do Noren
- [ ] Auto-update: baixa e instala nova versão no próximo reinício, sem ação do dono
- [ ] Impressão via USB e serial (impressora térmica 80mm, ex: Epson TM-T20X)

### Out of Scope

<!-- Limites explícitos com razão, pra não re-adicionar depois. -->

- Suporte a Mac — foco inicial só Windows, onde estão os clientes do Noren
- Múltiplas impressoras no mesmo agente — 1 agente = 1 impressora no v1, reduz complexidade
- Interface de configuração elaborada — o agente é invisível; só a tela de ativação basta
- Histórico de impressões (na UI do agente) — o Noren é a fonte de verdade dos jobs
- Impressoras de rede — só USB e serial por enquanto
- Renderização de ESC/POS no agente — o Noren renderiza os bytes (spooler burro), fonte única de verdade

## Context

**Origem:** substitui a dependência do QZ Tray no Noren (~/repos/personal/noren), que tem
limitações no plano gratuito. Brevly é o nome da empresa; este é um repo novo dentro de
~/repos/brevly.

**Como o Noren imprime hoje (investigado no código):**
- Impressão é iniciada no **navegador**: o cliente importa `qz-tray` dinamicamente e conecta
  em `ws://localhost:8181`, exigindo aba aberta no PC do caixa.
- Os bytes **ESC/POS são gerados no cliente** por funções puras em `src/lib/utils/ticket.ts`
  (`buildTicket`, `buildDespachoTicket`, `buildClosingTicket`, `buildCancelTicket`), com
  encoding ISO-8859-1 e QR nativo via comandos `GS(k`. Impressora alvo: Epson TM-T20X 80mm.
- O backend (SvelteKit ^2.63 / Vercel serverless / Postgres via Drizzle ^0.45 / Neon) já usa
  **Pusher** (^5.3.4 server, ^8.5.0 client) com canais privados por tenant
  (`private-tenant-${tenantId}-kitchen`), auth via Better Auth. Já grava flags de impressão
  no servidor (`kitchen_printed_at`, `dispatch_printed_at`).

**Implicação:** o Noren precisa de mudanças para (a) rodar os builders ESC/POS no servidor,
(b) enfileirar print jobs com endpoint HTTP autenticado para buscar bytes + ack, e
(c) emitir evento leve de print no Pusher. Essas mudanças no Noren são pré-requisito da
integração, mas ficam no repo do Noren — este roadmap cobre o agente Brevly Print.

**Eventos de impressão (confirmados no Noren):**
- Comanda de pedido — transição `novo → preparando` (operador aceita). Dados: itens, obs,
  addons, pagamento, endereço, nº pedido + hora.
- Comanda do entregador — transição `preparando → pronto` (entrega). Dados: endereço, itens,
  pagamento, `dispatchToken` de 6 chars renderizado como QR (`GS(k`) + código manual.
- Cupom de fechamento — aprovação do fechamento de caixa. Dados: sessão, operador, aprovador,
  reconciliação por método de pagamento, sangrias/reforços, acerto de entregadores.
- (Bônus existente: comanda de cancelamento, quando um pedido já impresso é cancelado.)

## Constraints

- **Tech stack**: Rust nativo — binário único enxuto, `tray-icon` (bandeja),
  `native-windows-gui` (tela de ativação), `serialport`/spooler USB para impressão. Sem
  webview. Escolhido por menor footprint, confiabilidade always-on e menor superfície de revisão.
- **Plataforma**: Windows apenas (v1). Impressoras USB e serial apenas.
- **Latência**: comanda na impressora em < 1 segundo após o evento.
- **Confiabilidade**: nenhuma comanda perdida — retry local (impressora offline) + fila
  server-side no Noren (agente offline/internet caiu).
- **Transporte**: Pusher para wakeup do evento + HTTP autenticado para payload/ack. Payload
  não vai pelo Pusher (limite ~10KB; cupom de fechamento pode estourar).
- **Renderização**: bytes ESC/POS gerados pelo Noren (spooler burro no agente) — fonte única
  de verdade dos templates, evita duplicar/portar layout e QR para Rust.
- **Ativação/licença**: serial gerado e validado pelo backend do Noren (serial → tenant).
- **Operação**: usuário-alvo é o dono/gerente (instala 1×); o caixa nunca interage.
- **Modo de trabalho**: desenvolvimento 100% via GSD; o dono do projeto apenas revisa.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust nativo (não Tauri) | Agente headless always-on; UI mínima não justifica webview; menor footprint e menor superfície de revisão | — Pending |
| Agente é spooler burro (Noren renderiza ESC/POS) | Uma fonte de verdade dos templates; agente trivial; sem drift de layout/QR em Rust | — Pending |
| Pusher (evento) + HTTP (payload) + ack | Evita limite de ~10KB do Pusher; dá confirmação de entrega e fila server-side p/ offline | — Pending |
| Serial gerado/validado pelo backend do Noren | Reusa auth/tenant existente; sem infra de licenciamento separada | — Pending |
| Fila de resiliência: retry local + pull de pendentes no reconnect | Cobre impressora offline e internet offline sem fila local complexa | — Pending |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd:complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-07-15 after initialization*
