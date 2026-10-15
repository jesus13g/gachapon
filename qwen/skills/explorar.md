---
descripcion: Localizar código en un repo desconocido sin gastar contexto. Útil casi siempre que la tarea no nombre todos los ficheros.
---
Tu contexto es pequeño: cada fichero leído entero lo consume.
- Empieza por la estructura: lista directorios con `ls` o `find . -maxdepth 2 -not -path '*/.git*'`.
- Busca antes de leer: `grep -rn "nombre" --include=*.rs .` (o `rg -n` si existe). Busca el símbolo exacto que nombra la tarea.
- Lee solo el fragmento útil: `sed -n '120,180p' fichero` en vez del fichero completo. Lee entero solo si tiene menos de 200 líneas.
- No abras `target/`, `node_modules/`, `.venv/`, `dist/` ni ficheros generados o binarios.
- Antes de cambiar una función, busca quién la llama (`grep -rn "funcion("`).
- Deja de explorar en cuanto sepas qué fichero y qué líneas tocar.
