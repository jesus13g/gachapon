---
descripcion: Reorganizar código sin cambiar su comportamiento (renombrar, extraer funciones, mover módulos).
---
Sigue estos pasos en orden:
1. Lista TODAS las apariciones, también en tests y documentación (.md): `grep -rn 'viejo' --exclude-dir=.git .`
2. Si es un renombrado, cámbialas todas en un comando: `grep -rlw 'viejo' --exclude-dir=.git . | xargs sed -i 's/\bviejo\b/nuevo/g'`. Si no es mecánico, edita fichero a fichero.
3. Repite el grep del paso 1: debe salir vacío. Si queda algo (README, docs, comentarios), cámbialo.
4. Ejecuta los tests: deben pasar igual que antes. No mezcles arreglos ni funciones nuevas.
