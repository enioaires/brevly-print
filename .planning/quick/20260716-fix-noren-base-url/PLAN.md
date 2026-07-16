---
quick_id: 20260716-fix-noren-base-url
slug: fix-noren-base-url
date: 2026-07-16
status: complete
---

# Fix NOREN_BASE_URL_DEFAULT

## Goal

Corrigir o domínio padrão do Noren em `src/noren_client.rs`.

O valor atual `https://app.noren.com.br` está errado — o domínio real de produção
é `https://noren.app.br` (confirmado em `noren/.env` → `PUBLIC_APP_URL`).

## Must-Haves

- [ ] `NOREN_BASE_URL_DEFAULT` alterado para `"https://noren.app.br"`

## Tasks

1. Editar `src/noren_client.rs` linha 19: substituir `"https://app.noren.com.br"` por `"https://noren.app.br"`
2. Commit atômico
