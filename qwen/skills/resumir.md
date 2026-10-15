---
descripcion: Sacar datos de ficheros grandes, logs o salidas de comandos y dejar un resumen en un fichero.
---
NO uses read_file con el fichero grande: no cabe en tu contexto. Trabaja con run_shell_command:
- Tamaño y forma: `wc -l f && head -5 f`.
- Cada dato con UN comando y copia el número de su salida. Filtra ANTES de agrupar o contar:
  - contar: `grep -c ' ERROR ' f`
  - primera aparición: `grep -m1 ' ERROR ' f`
  - más frecuente entre las líneas filtradas: `grep ' ERROR ' f | grep -o '\[[^]]*\]' | sort | uniq -c | sort -rn | head -3`
- Escribe el resumen de una vez con `cat > resumen.md <<'EOF'`: título y una línea por dato, con el valor exacto.
- No inventes: si un dato no sale de un comando, dilo.
