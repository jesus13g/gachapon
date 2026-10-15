---
descripcion: Escribir scripts de bash/sh robustos o automatizar con comandos de shell.
---
- Cabecera: `#!/usr/bin/env bash` y `set -euo pipefail`.
- Entrecomilla siempre las variables: `"$var"`, `"$@"`.
- Comprueba argumentos al principio y muestra un uso breve si faltan (`exit 2`).
- Ficheros temporales con `mktemp` y limpieza con `trap 'rm -f "$tmp"' EXIT`.
- Usa herramientas estándar (grep, sed, awk, find). No asumas que existe nada que no veas con `command -v`.
- Errores a stderr (`>&2`) y código de salida distinto de 0.
- Haz el script ejecutable (`chmod +x`) y pruébalo con `bash -n script` y una ejecución real.
