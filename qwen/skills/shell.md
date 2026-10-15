---
descripcion: Escribir scripts de bash/sh robustos o automatizar con comandos de shell.
---
- Crea el script de una vez con `cat > s.sh <<'EOF'` y `chmod +x s.sh`.
- Cabecera `#!/usr/bin/env bash` y `set -euo pipefail`; entrecomilla siempre `"$var"`.
- Argumentos: compruébalos al principio; el uso va a stderr (`>&2`) con `exit 2`.
- Recorrer ficheros con espacios: `find "$d" -type f -print0 | while IFS= read -r -d '' f; do …; done`.
- Trabaja sobre el nombre, no la ruta: `b=$(basename "$f")`; extensión `${b##*.}` solo si `[[ $b == *.* ]]`.
- Prueba en un directorio de `mktemp -d` (su ruta tiene puntos) con nombres con espacios y sin extensión, no solo con el ejemplo de la tarea.
