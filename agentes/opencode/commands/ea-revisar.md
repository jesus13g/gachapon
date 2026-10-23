---
description: "Revisa los trabajos que el ejecutor ha entregado al planificador (cola del planificador): misiones, obras y rutinas. Lee el resumen, abre el diff solo si hace falta y decide cerrar, reformular o continuar. Úsala cuando el usuario pida revisar, o tras lanzar una misión, obra o rutina."
---

Procedimiento completo y vigente: **lee `~/Proyectos/ecosistema-asistente/agentes/flujo/revisar.md` (en la raíz del repo, `agentes/flujo/revisar.md`) y síguelo.** Es la guía neutral que comparten todos los planificadores; este fichero es solo su adaptador.

Reglas que no se rompen:

- El planificador **formaliza y revisa**; no implementa lo que puede hacer el ejecutor. `ea agentes` dice quién ocupa cada rol y con qué modelo.
- **Ningún agente se confirma a sí mismo:** lo mutante se simula o se propone y solo el usuario acepta.
- El criterio de éxito lo ejecuta el sistema; un criterio vacío no es éxito.

Resumen: `ea cola planificador --json`, `ea trabajo ver <id>`, `ea trabajo diff <id>` solo si hace falta, y decidir: `cerrar`, `reformular --nota …` (con `--fase N` si el fallo es anterior), `continuar` o `cancelar`. Que el criterio pase no basta; no rehagas el trabajo.

Petición del usuario: $ARGUMENTS
