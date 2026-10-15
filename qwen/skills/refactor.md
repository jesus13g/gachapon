---
descripcion: Reorganizar código sin cambiar su comportamiento (renombrar, extraer funciones, mover módulos).
---
- El comportamiento no cambia: los tests que pasaban deben seguir pasando. Ejecútalos ANTES de empezar para conocer el punto de partida.
- Pasos pequeños: un cambio, compilar/ejecutar tests, siguiente cambio.
- Al renombrar o mover, busca todas las referencias (`grep -rn "nombre_viejo"`) y actualízalas todas, también en tests y documentación.
- No mezcles arreglos de bugs ni funcionalidades nuevas con el refactor.
- No dejes código muerto, imports sin usar ni funciones duplicadas.
