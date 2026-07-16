---
slug: fix-static-crt
created: "2026-07-16"
status: in-progress
---

# Fix: Static CRT linking for Windows binary

## Goal

Eliminar dependência de `VCRUNTIME140.dll` no binário Windows. Atualmente compilado com
`x86_64-pc-windows-msvc` sem CRT estático, causando erro de instalação em VMs/máquinas
sem o Visual C++ Redistributable instalado.

## Root Cause

Binário usa dynamic linking do MSVC CRT por padrão. Máquinas sem VC++ Redist instalado
recebem: "VCRUNTIME140.dll was not found."

## Task

1. Criar `.cargo/config.toml` com `target-feature=+crt-static` para o target MSVC:

```toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

2. Commit: `fix: statically link MSVC CRT to eliminate VCRUNTIME140.dll dependency`
