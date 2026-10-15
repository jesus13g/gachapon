---
descripcion: Localizar código en un repo desconocido sin gastar contexto. Útil cuando la tarea no nombra los ficheros a tocar.
---
Cada turno y cada línea leída consumen tu contexto. Encuentra el sitio con pocos comandos:
- Un solo comando para orientarte: `find . -name '*.py' -not -path './.git/*' | xargs wc -l | sort -n` (cambia la extensión). Ves ficheros y tamaños a la vez.
- Busca el concepto antes de leer: `grep -rn -i 'palabra' --include='*.py' . | grep -v '^./ruta/ruidosa' | head -30`.
- read_file solo con ficheros de menos de 200 líneas. Para los grandes: `sed -n '120,180p' fichero` o `grep -n patrón fichero`.
- Ignora datos, fixtures, catálogos y ficheros generados: casi nunca contienen la lógica.
- Agrupa: varias lecturas pequeñas en un comando (`cat a.py b.py`), varias comprobaciones con `&&`.
- Para en cuanto sepas qué fichero y líneas cambiar.
