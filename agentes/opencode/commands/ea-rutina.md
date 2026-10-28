---
description: "Formaliza un proceso repetible como rutina versionada (TOML), la registra con `ea rutina registrar`, la lanza o la programa con un timer de systemd. También corrige una rutina cuando llega un paquete de corrección tras un fallo. Úsala para procesos que se repetirán."
---

Procedimiento completo y vigente: **lee `{{flujo}}/rutina.md` (en la raíz del repo, `agentes/flujo/rutina.md`) y síguelo.** Es la guía neutral que comparten todos los planificadores; este fichero es solo su adaptador.

Reglas que no se rompen:

- El planificador **formaliza y revisa**; no implementa lo que puede hacer el ejecutor. `ea agentes` dice quién ocupa cada rol y con qué modelo.
- **Ningún agente se confirma a sí mismo:** lo mutante se simula o se propone y solo el usuario acepta.
- El criterio de éxito lo ejecuta el sistema; un criterio vacío no es éxito.

Resumen: Escribir el TOML de la rutina, `ea rutina registrar` (solo la v1; una versión nueva se propone con `ea_registrar_rutina`), `ea rutina lanzar`; el horario y los cambios por MCP (`ea_programar_rutina`, `ea_registrar_rutina`…) primero con `simular: true` y luego como propuesta que acepta el usuario; y la corrección de un paquete de fallo.

Petición del usuario: $ARGUMENTS
