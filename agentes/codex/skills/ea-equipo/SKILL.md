---
name: ea-equipo
description: "Trabaja dentro de un equipo de agentes despertado por el usuario: el planificador parte el objetivo en misiones u obras y las reparte entre los ejecutores del equipo con `ea trabajo crear --equipo --asignar`; el revisor revisa las entregas y propone cerrar o reformular. Úsala cuando el prompt o el usuario mencione un equipo (`ea equipo ver`)."
---

# ea-equipo

Procedimiento completo y vigente: **lee `~/Proyectos/ecosistema-asistente/agentes/flujo/equipo.md` (en la raíz del repo, `agentes/flujo/equipo.md`) y síguelo.** Es la guía neutral que comparten todos los planificadores y revisores; este fichero es solo su adaptador.

Reglas que no se rompen:

- Despertar y dormir equipos es **solo del usuario**. El planificador parte y reparte, **no implementa**; el revisor **solo propone** (cerrar o reformular lo hace el planificador o el usuario) y nunca revisa lo que ejecutó él.
- **Ningún agente se confirma a sí mismo:** lo mutante se simula o se propone y solo el usuario acepta. El criterio de éxito lo ejecuta el sistema; un criterio vacío no es éxito.

Resumen: leer `ea equipo ver <id>`, partir el objetivo en misiones u obras con criterio comprobable (`agentes/flujo/mision.md`, `obra.md`), repartirlas con `ea trabajo crear --equipo <id> --asignar <ejecutor> [--revisor <otro>] --worktree-propio` (el worktree propio cuando varias toquen el mismo repo) y vigilar con `ea equipo ver`. El revisor lee su cola de revisión y revisa como `agentes/flujo/revisar.md`. No esperes en bucle.
