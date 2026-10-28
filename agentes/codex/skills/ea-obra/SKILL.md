---
name: ea-obra
description: "Lanza una obra al ejecutor: un plan dividido en fases pequeñas, cada una con instrucciones, entregable y criterio de comando, que se implementa fase a fase con revisión del planificador cada N fases. Úsala cuando el usuario tenga un plan o una tarea grande que implementar. El planificador solo formaliza."
---

# ea-obra

Procedimiento completo y vigente: **lee `{{flujo}}/obra.md` (en la raíz del repo, `agentes/flujo/obra.md`) y síguelo.** Es la guía neutral que comparten todos los planificadores; este fichero es solo su adaptador.

Reglas que no se rompen:

- El planificador **formaliza y revisa**; no implementa lo que puede hacer el ejecutor. `ea agentes` dice quién ocupa cada rol y con qué modelo.
- **Ningún agente se confirma a sí mismo:** lo mutante se simula o se propone y solo el usuario acepta.
- El criterio de éxito lo ejecuta el sistema; un criterio vacío no es éxito.

Resumen: Dividir el plan en fases pequeñas (`numero`, `titulo`, `instrucciones`, `entregable`, `criterio`), preguntar `revisar_cada` si la obra es mayor, escribir la orden JSON (`creada_por: "planificador"`) y encolarla con `ea trabajo crear --orden …`.
