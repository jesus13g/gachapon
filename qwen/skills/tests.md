---
descripcion: Escribir tests que comprueben de verdad el comportamiento pedido, o hacer pasar los existentes.
---
- Ejecuta primero el criterio de éxito para ver qué falla y por qué.
- Un test por comportamiento: nombre descriptivo, datos mínimos y una aserción clara del resultado esperado.
- Cubre el caso normal, un caso límite (vacío, cero, último elemento) y un caso de error si la tarea lo menciona.
- NUNCA cambies, borres o debilites un test existente para que pase; arregla el código. Solo modifica un test si la tarea lo pide.
- No uses `sleep`, red ni rutas absolutas en tests; usa directorios temporales.
- Tras cada cambio vuelve a ejecutar el test concreto, y al final la batería completa del módulo.
