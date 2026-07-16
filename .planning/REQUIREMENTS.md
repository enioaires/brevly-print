# Requirements: Brevly Print

**Defined:** 2026-07-15
**Core Value:** Quando um evento de impressão chega do Noren, a comanda/cupom correto sai na
impressora térmica em < 1 segundo, de forma confiável e sem intervenção humana — nenhuma
comanda perdida.

## v1 Requirements

Requisitos do release inicial. Cada um mapeia para uma fase do roadmap.

### Ativação & Setup

- [ ] **ACT-01**: Instalador Windows baixável pelo site instala o agente como programa comum
- [ ] **ACT-02**: Na primeira execução, o agente abre uma tela pedindo o serial number
- [ ] **ACT-03**: O serial é validado contra o backend do Noren, vinculando o agente ao restaurante (tenant)
- [ ] **ACT-04**: O dono seleciona a impressora de uma lista combinada (impressoras Windows + portas COM)
- [ ] **ACT-05**: Botão de teste de impressão valida que os bytes RAW chegam à impressora antes de salvar
- [ ] **ACT-06**: Ao salvar, o agentToken é armazenado criptografado (DPAPI) e a config persistida (SQLite)
- [ ] **ACT-07**: Se a credencial ficar ilegível (ex: reinstalação do Windows), o agente retorna ao fluxo de ativação
- [ ] **ACT-08**: O agente registra autostart e inicia automaticamente com o Windows

### Runtime & Bandeja

- [ ] **RUN-01**: O agente roda invisível, sem UI além do ícone na bandeja durante operação normal
- [ ] **RUN-02**: O ícone na bandeja indica estado: verde = conectado, amarelo = reconectando, vermelho = problema
- [ ] **RUN-03**: O agente sobrevive a reboot e reconecta automaticamente sem intervenção

### Recebimento de Eventos

- [ ] **EVT-01**: O agente conecta ao Pusher (canal privado por tenant) e recebe eventos de impressão `{jobId, type}`
- [ ] **EVT-02**: A auth de canal é refeita a cada reconexão com o socket_id novo
- [ ] **EVT-03**: Health check ping/pong detecta conexão zumbi e força reconexão com backoff exponencial

### Pipeline de Impressão

- [ ] **PRT-01**: Ao receber um evento, o agente busca os bytes ESC/POS do job via HTTP autenticado
- [ ] **PRT-02**: O agente imprime a comanda de pedido (pedido novo confirmado)
- [ ] **PRT-03**: O agente imprime a comanda do entregador, com o QR de despacho (já embutido nos bytes do Noren)
- [ ] **PRT-04**: O agente imprime o cupom de fechamento de caixa
- [ ] **PRT-05**: O agente imprime via impressora Windows (WritePrinter datatype RAW) e via porta serial/COM
- [ ] **PRT-06**: A comanda sai na impressora em menos de 1 segundo após o evento chegar
- [ ] **PRT-07**: Dedup persistente (SQLite) impede impressão dupla em reconexão/redelivery/crash
- [ ] **PRT-08**: O ack de entrega é enviado somente após a impressão confirmada (grava `done` antes do ack)
- [ ] **PRT-09**: O agente respeita as flags de habilitação por tipo de impressão (fonte: config vinda do Noren)

### Resiliência

- [ ] **RES-01**: O agente retenta a impressão 3× com intervalo de 30s quando a impressora falha
- [ ] **RES-02**: Notificação do Windows (linguagem simples) + ícone vermelho quando as retentativas se esgotam
- [ ] **RES-03**: Ao reconectar após queda de internet, o agente puxa os jobs pendentes do Noren (fila server-side) — nenhuma comanda perdida
- [ ] **RES-04**: Recuperação no boot: jobs deixados em status `printing` são reprocessados com dedup

### Distribuição & Atualização

- [ ] **DIST-01**: O instalador é assinado (Authenticode) desde o primeiro release
- [x] **DIST-02**: Auto-update — o agente baixa e instala a nova versão automaticamente no próximo reinício, sem ação do dono
- [x] **DIST-03**: Verificação de integridade (SHA256) do binário antes de aplicar qualquer update

## v2 Requirements

Reconhecidos, mas fora do roadmap atual.

### Observabilidade

- **OBS-01**: Heartbeat periódico ao Noren para o dashboard indicar "impressora online/offline" e detectar quarentena por antivírus
- **OBS-02**: Reporte de status da impressora (papel acabando) via ESC/POS `DLE EOT` — só confiável em modo serial

### Preferências

- **PREF-01**: UI de toggle por tipo de impressão no dashboard do Noren (lado Noren; o agente já respeita a flag no v1)

## Out of Scope

Excluídos explicitamente para evitar scope creep.

| Feature | Reason |
|---------|--------|
| Suporte a Mac | Foco inicial só Windows, onde estão os clientes do Noren |
| Múltiplas impressoras no mesmo agente | 1 agente = 1 impressora no v1; reduz complexidade |
| Interface de configuração elaborada | O agente é invisível; só a tela de ativação basta |
| Histórico de impressões na UI do agente | O Noren é a fonte de verdade dos jobs |
| Impressoras de rede | Só USB e serial por enquanto |
| Renderização de ESC/POS no agente | O Noren renderiza os bytes (spooler burro), fonte única de verdade |
| Acesso USB direto via WinUSB/Zadig (`escpos`/`CreateFile`) | Exige troca de driver; `WritePrinter` RAW cobre o alvo (TM-T20X) |
| Gaveta de dinheiro (cash drawer kick) | Não faz parte do fluxo do Noren no v1 |

## External Dependencies (Noren backend — repo separado)

Estas mudanças vivem em `~/repos/brevly/noren` e são **pré-requisito** para as fases de
integração. O roadmap deve sequenciar as fases do agente considerando que o endpoint
correspondente exista (ou seja construído em paralelo) antes da fase que o consome.

| Mudança no Noren | Habilita | Bloqueia fase (agente) |
|---|---|---|
| Renderização de ESC/POS server-side (migrar `buildTicket`/`buildDespachoTicket`/`buildClosingTicket` de `ticket.ts`), preservando ISO-8859-1 | Agente receber bytes | Pipeline de impressão (Phase 5) |
| Tabelas `agent_serials` + `agent_print_jobs` | Serial auth + fila | Ativação (Phase 2), Pipeline (Phase 5) |
| `POST /api/agent/activate` (valida serial, emite agentToken, suporta re-ativação) | Ativação | Ativação (Phase 2) |
| `POST /api/agent/pusher/auth` (HMAC + 403 se canal ≠ tenant do token) | Subscrição Pusher | Eventos (Phase 4) |
| Emissão de evento Pusher leve `{jobId, type}` em pedido/despacho/fechamento | Receber eventos | Eventos (Phase 4) |
| `GET /api/agent/jobs/{jobId}/bytes` (base64 ESC/POS) | Buscar job | Pipeline (Phase 5) |
| `POST /api/agent/jobs/{jobId}/ack` (idempotente, 409 no repeat) | Ack + dedup | Pipeline (Phase 5) |
| `GET /api/agent/jobs/pending` (não-ackados, ASC, máx 100) | Pull offline | Resiliência (Phase 6) |
| `GET /api/agent/version` (versão + downloadUrl + SHA256) + hosting do update | Auto-update | Distribuição (Phase 7) |

## Traceability

Preenchido durante a criação do roadmap.

| Requirement | Phase | Status |
|-------------|-------|--------|
| ACT-01 | Phase 2 | Pending |
| ACT-02 | Phase 2 | Pending |
| ACT-03 | Phase 2 | Pending |
| ACT-04 | Phase 2 | Pending |
| ACT-05 | Phase 2 | Pending |
| ACT-06 | Phase 2 | Pending |
| ACT-07 | Phase 2 | Pending |
| ACT-08 | Phase 2 | Pending |
| RUN-01 | Phase 3 | Pending |
| RUN-02 | Phase 3 | Pending |
| RUN-03 | Phase 3 | Pending |
| EVT-01 | Phase 4 | Pending |
| EVT-02 | Phase 4 | Pending |
| EVT-03 | Phase 4 | Pending |
| PRT-01 | Phase 5 | Pending |
| PRT-02 | Phase 5 | Pending |
| PRT-03 | Phase 5 | Pending |
| PRT-04 | Phase 5 | Pending |
| PRT-05 | Phase 5 | Pending |
| PRT-06 | Phase 5 | Pending |
| PRT-07 | Phase 5 | Pending |
| PRT-08 | Phase 5 | Pending |
| PRT-09 | Phase 5 | Pending |
| RES-01 | Phase 6 | Pending |
| RES-02 | Phase 6 | Pending |
| RES-03 | Phase 6 | Pending |
| RES-04 | Phase 6 | Pending |
| DIST-01 | Phase 3 | Pending |
| DIST-02 | Phase 7 | Complete |
| DIST-03 | Phase 7 | Complete |

**Coverage:**
- v1 requirements: 25 total
- Mapped to phases: 25
- Unmapped: 0

---
*Requirements defined: 2026-07-15*
*Last updated: 2026-07-15 after roadmap creation (traceability filled)*
