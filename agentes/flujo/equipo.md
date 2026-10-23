# Flujo · equipo

Procedimiento neutral para trabajar dentro de un **equipo** (spec 019): un grupo
de agentes que el usuario ha despertado para una tarea, con tres roles:
`planificador` (parte y reparte), `ejecutor` (ejecuta, en paralelo con los
demás) y `revisor` (un planificador que solo revisa). Vale para cualquier agente
que haya sido despertado como planificador o revisor; el prompt de la sesión
dice cuál eres y el id del equipo.

## Reglas

- **Despertar y dormir equipos es del usuario** (terminal real o panel con
  clave). Tú no creas, despiertas ni duermes equipos.
- El planificador **parte y reparte; no implementa.** Lo que se puede escribir
  como pasos con un criterio comprobable lo hace un ejecutor del equipo.
- **Ningún agente se confirma a sí mismo ni confirma a otro.** El criterio de
  éxito lo ejecuta el sistema; un criterio vacío no es éxito. Lo mutante se
  simula o se propone, y solo el usuario acepta.
- Un trabajo nunca lo revisa quien lo ejecutó, y un revisor **solo propone**:
  no cierra ni reformula.
- Tú ocupas un solo rol dentro del equipo; no hagas el de otro.

## Antes de empezar

`ea equipo ver <id>` (o `--json`) da el objetivo, el repo base (`worktree_base`),
los miembros con su rol, si están despiertos y en qué trabajan, las colas y los
trabajos del equipo con su último informe. `ea agentes` dice el modelo de cada
uno: dimensiona cada trabajo para el ejecutor al que lo vas a asignar. Si un
ejecutor figura dormido o «no disponible», no le asignes nada: esperaría en su
cola y nunca pasa a otro por su cuenta.

## Planificador: repartir

1. **Parte el objetivo** en misiones u obras independientes, cada una con su
   `objetivo` concreto y un `criterio_exito` de un comando de shell. Cómo
   formalizar cada una está en `agentes/flujo/mision.md` (y
   `agentes/flujo/obra.md` si es grande): no lo repitas aquí.
2. **Reparte** con la CLI (el id del equipo y los agentes salen de
   `ea equipo ver`):

```bash
ea trabajo crear --modo mision --titulo T --worktree <repo-del-equipo> \
  --objetivo O --criterio C --skills rust,tests --por planificador \
  --equipo <id> --asignar <ejecutor> [--revisor <otro-agente>] --worktree-propio
```

   - `--asignar <ejecutor>`: el trabajo va a la cola de ese agente y solo él lo
     toma. Sin `--asignar`, lo toma el primer ejecutor libre del equipo.
   - `--worktree-propio`: pónlo **siempre que varios trabajos toquen el mismo
     repo**; el sistema crea un `git worktree` (rama `ea/<id>`) por trabajo y
     así corren a la vez sin pisarse. Sin él, los trabajos sobre la misma ruta
     se serializan por el semáforo.
   - `--revisor <agente>`: debe ser un revisor o planificador del equipo y
     **distinto del ejecutor asignado**. La entrega llega a su cola y, siempre,
     también a la del planificador.
3. **No esperes en bucle ni implementes mientras tanto.** Informa de los ids y
   vigila con `ea equipo ver <id>` (presencia, trabajo actual, entregas) o
   `ea status`.
4. **Revisa lo que llegue a tu cola** como en `agentes/flujo/revisar.md`
   (`ea cola planificador`): cerrar, reformular o continuar. Cada rama
   `ea/<id>` es del usuario: **fusionarlas no es cosa tuya**, dilo al terminar.
5. Si reformulaste dos veces sin avance, díselo al usuario y propón partir el
   trabajo o cambiar de ejecutor.

## Revisor: revisar y proponer

1. Tu cola de revisión sale en tu fila de `ea equipo ver <id>`; cada entrada
   es un trabajo entregado con tu id como `revisor`.
2. Por cada trabajo, revisa como en `agentes/flujo/revisar.md`: `ea trabajo ver
   <id>`, y `ea trabajo diff <id>` solo si el resumen no basta. Que el criterio
   pase no basta: comprueba que el trabajo es correcto de verdad.
3. **Propón, no decidas:** di en tu respuesta al usuario si cerrarías o
   reformularías y con qué nota concreta (fichero, función, qué falla, qué
   comando probar). El que cierra o reformula es el planificador o el usuario.
4. Si el trabajo lo ejecutó tu mismo agente, no lo revises: avisa. Una entrega
   cuyo revisor coincide con quien ejecutó solo va a la cola del planificador
   y el informe lleva un aviso.

## Herramientas MCP

`ea-mcp` da lectura: `ea_listar_equipos`, `ea_ver_equipo` y `ea_status` (con las
presencias), además de `ea_listar_trabajos` y `ea_ver_trabajo`. **Crear
trabajo, decidirlo y despertar o dormir equipos es de la CLI** (lo último,
además, solo con TTY o con la clave del panel).
