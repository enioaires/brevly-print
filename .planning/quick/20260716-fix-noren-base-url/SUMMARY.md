---
quick_id: 20260716-fix-noren-base-url
slug: fix-noren-base-url
date: 2026-07-16
status: complete
---

# Summary: Fix NOREN_BASE_URL_DEFAULT

## What was done

Corrigido `NOREN_BASE_URL_DEFAULT` em `src/noren_client.rs` de `"https://app.noren.com.br"`
para `"https://noren.app.br"` — domínio real de produção do Noren.

A URL errada causava erro "Sem conexão com o servidor" na janela de ativação
porque o servidor não existia naquele endereço.

## Files changed

- `src/noren_client.rs` — linha 19: corrigido domínio padrão
