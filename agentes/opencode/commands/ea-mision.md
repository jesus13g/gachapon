---
description: "Lanza una misión al ejecutor para alcanzar un objetivo iterando hasta que un comando de shell pase. Úsala cuando el usuario pida «una misión» o un trabajo que se pueda describir en pasos verificables (crear o arreglar código, buscar, leer, resumir). El planificador solo formaliza; no lo implementa."
---

Procedimiento completo y vigente: **lee `{{flujo}}/mision.md` (en la raíz del repo, `agentes/flujo/mision.md`) y síguelo.** Es la guía neutral que comparten todos los planificadores; este fichero es solo su adaptador.

Reglas que no se rompen:

- El planificador **formaliza y revisa**; no implementa lo que puede hacer el ejecutor. `ea agentes` dice quién ocupa cada rol y con qué modelo.
- **Ningún agente se confirma a sí mismo:** lo mutante se simula o se propone y solo el usuario acepta.
- El criterio de éxito lo ejecuta el sistema; un criterio vacío no es éxito.

Resumen: Formalizar un objetivo con un criterio de shell comprobable, escribir la orden JSON y encolarla con `ea trabajo crear --orden …` (o la forma rápida con `--por planificador`), elegir las skills mínimas del ejecutor (`ea qwen-skills listar`) e informar del id. No implementes ni esperes en bucle.

Petición del usuario: $ARGUMENTS
