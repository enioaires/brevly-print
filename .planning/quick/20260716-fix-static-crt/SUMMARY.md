---
slug: fix-static-crt
status: complete
completed: "2026-07-16"
---

# Summary: Static CRT linking para Windows

Criado `.cargo/config.toml` com `target-feature=+crt-static` para `x86_64-pc-windows-msvc`.

Elimina dependência de `VCRUNTIME140.dll` — binário agora carrega o MSVC runtime
estaticamente, sem precisar do Visual C++ Redistributable na máquina de destino.

Commit: 66a0499
