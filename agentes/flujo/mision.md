# Flujo · misión

Procedimiento neutral del **planificador** para lanzar una misión: alcanzar un
objetivo iterando hasta que un comando de shell pase. Vale para cualquier agente
que ocupe el rol de planificador (`ea agentes` dice quién es).

## Reglas

- El planificador **formaliza y encola**; no implementa el trabajo, ni siquiera
  «para ir más rápido». Si se puede describir como pasos verificables, lo hace el
  ejecutor.
- **Ningún agente se confirma a sí mismo.** El criterio de éxito lo ejecuta el
  sistema, nunca el agente que trabaja, y un criterio vacío no es éxito.
- Lo que cambia el sistema se simula o se propone; solo el usuario acepta.

## Antes de empezar

`ea agentes` dice qué agente y qué modelo ocupan cada rol. Dimensiona la tarea
para el ejecutor **actual**: un modelo local pequeño necesita pasos concretos y
pocos ficheros.

## Cuándo NO usarla

- Requiere diseño, arquitectura o depuración difícil: hazlo tú.
- Es demasiado grande para el ejecutor (muchos ficheros, decisiones abiertas):
  propón una obra con fases pequeñas (`agentes/flujo/obra.md`).

## Pasos

1. **Worktree.** Ruta absoluta de un repo git (por defecto el directorio actual;
   comprueba `git -C <ruta> rev-parse --show-toplevel`). Si
   `git status --porcelain` muestra cambios sin commit, avisa al usuario: el
   diff final los mezclaría.
2. **Formaliza** y enséñale al usuario el resultado en pocas líneas:
   - `objetivo`: texto concreto para el ejecutor, con ficheros y funciones
     nombrados, firmas y comportamiento esperado. Sin ambigüedad.
   - `criterio_exito`: UN comando de shell que pase o falle, ejecutado desde el
     worktree: tests (`python test_x.py`, `cargo test -p x`), `test -f ruta`,
     `grep -q patrón fichero` o un script. Debe fallar ahora y pasar solo si el
     objetivo se cumple. Si no existe, el objetivo incluye crear el test, pero
     es preferible escribirlo tú antes si es corto (eso es formalizar, no
     implementar).
   - `skills`: las skills del ejecutor que necesita la tarea (ver más abajo).
   - `topes`: `iteraciones` 3-5 (por defecto 5); `tiempo_s` 900 por iteración;
     `turnos` 30; `llamadas_herramienta` 60. Sube solo con motivo: el sistema
     recorta lo que pase del techo de los ajustes (`ea ajustes ver`, sección
     agente). Algunos ejecutores solo respetan el tiempo.
3. **Escribe la orden** en un temporal (`mktemp`), con `id` y `creada_en`
   vacíos (se rellenan solos):

```json
{
  "id": "",
  "modo": "mision",
  "titulo": "título corto",
  "worktree": "/ruta/absoluta",
  "objetivo": "…",
  "criterio_exito": "python test_calc.py",
  "skills": ["python", "tests"],
  "topes": {"turnos": 30, "tiempo_s": 900, "llamadas_herramienta": 60, "iteraciones": 4},
  "creada_en": "",
  "creada_por": "planificador"
}
```

4. **Encola:** `ea trabajo crear --orden /tmp/…json` imprime el **id**.
5. **Informa** del id y de que llegará a la cola del planificador al terminar o
   bloquearse (`ea cola planificador`). La revisión sigue
   `agentes/flujo/revisar.md`. No esperes en bucle ni hagas el trabajo mientras
   tanto.

Forma rápida, sin JSON:

```bash
ea trabajo crear --modo mision --titulo T --worktree . --objetivo O \
  --criterio C --skills python,tests --por planificador
```

## Skills del ejecutor

El ejecutor no carga nada por su cuenta: solo las skills que pongas en la orden
se añaden a su prompt. Son suyas, no las del planificador. La llave para
elegirlas la tiene el rol planificador (`creada_por: "planificador"`, o el id del
agente que lo ocupa).

- Catálogo: `ea qwen-skills listar` (el comando conserva su nombre histórico; sirve para cualquier ejecutor; nombre, tamaño y descripción; las marcadas
  con `x` están deshabilitadas).
- Elige **las mínimas** (0-2 normalmente; el máximo lo dice el listado): el
  contexto de un modelo local es pequeño. Típico: lenguaje (`rust`, `python`,
  `shell`) más tipo de trabajo (`tests`, `depurar`, `refactor`, `documentar`,
  `resumir`); `explorar` solo si el objetivo no nombra los ficheros.
- Si dudas de una, léela: `ea qwen-skills ver <nombre>`. Comprueba la selección
  con `ea qwen-skills probar a,b`.
- Si ninguna encaja, no pongas ninguna: un objetivo bien escrito vale más que
  una skill genérica.

## Herramientas MCP

`ea-mcp` da lectura del bus (`ea_status`, `ea_listar_trabajos`,
`ea_ver_trabajo`, `ea_listar_diario`). **Crear o decidir trabajo es de la CLI**,
no del MCP.
