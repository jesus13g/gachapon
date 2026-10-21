# Flujo · revisar

Procedimiento neutral del **planificador** para revisar los trabajos que el
ejecutor ha entregado (cola del planificador): misiones, obras y rutinas. El
planificador **decide**, no rehace. Leer de más cuesta cuota: el resumen debe
bastar casi siempre.

## Reglas

- **Que el criterio pase no basta.** Lo ejecutó el sistema, pero el planificador
  comprueba que el trabajo es correcto de verdad: la evidencia es real y no hay
  trampas (tests vaciados, criterio trivial).
- Ningún agente se confirma a sí mismo: no cierres un trabajo que tú mismo hiciste
  ni que ejecutó el agente que ocupa tu rol (el sistema lo impide al configurar
  los roles).
- Un criterio vacío no es éxito.

## Flujo

1. `ea cola planificador --json` lista las entradas (`trabajo`, `motivo`). Vacía:
   dilo y termina.
2. Por cada trabajo: `ea trabajo ver <id>` (modo, estado, iteración o fase,
   resultado, motivo, evidencia, diff stat, `obra_completa`, `ejecutado_por`).
   Con las herramientas MCP de solo lectura: `ea_ver_trabajo`,
   `ea_listar_trabajos`. Si hay muchos, uno a uno.
3. **Abre el diff solo si el resumen no basta:** `ea trabajo diff <id>`
   (completo, con ficheros nuevos) o `ea trabajo diff <id> --fase N` (una fase;
   necesita las instantáneas del store). Lee solo la parte necesaria (`| head`,
   `| sed -n`, o un fichero concreto).
4. **Decide:**
   - **Cerrar** (`ea trabajo cerrar <id>`): resultado `Terminado` y además
     correcto de verdad (el diff stat toca lo esperado, la salida del criterio es
     real).
   - **Reformular** (`ea trabajo reformular <id> --nota "…"`): falló, quedó
     `Bloqueado` o se queda corto. La nota va al ejecutor: instrucciones
     concretas (qué fichero, qué función, qué falla, qué comando probar). Nada de
     «hazlo mejor». En una obra se rehace la fase pendiente (en una obra
     completa, la última); si el fallo está en una fase anterior, añade
     `--fase N`: esa y las siguientes vuelven a quedar pendientes.
   - **Continuar** (`ea trabajo continuar <id>`): obra con
     `obra_completa: false` y resultado `Terminado` en una revisión intermedia;
     las fases hechas están bien. Si una fase hecha está mal, reformula con nota.
   - **Corrección de rutina** (existe `paquete-correccion.md` en el directorio
     del trabajo): sigue la sección «Corrección» de `agentes/flujo/rutina.md`.
   - **Cancelar** (`ea trabajo cancelar <id>`) solo si el usuario lo pide o el
     objetivo ya no tiene sentido.
5. **No rehagas el trabajo tú** salvo que lo exija criterio (diseño o depuración
   difícil que el ejecutor no puede). Si reformulaste ya dos veces sin avance,
   dilo al usuario y propón qué hacer (otro ejecutor con `ea agentes`, partir el
   trabajo, hacerlo tú).
6. **Resumen final**, una línea por trabajo:
   `<id> · <título> · cerrado|reformulado|continuado · motivo breve`.

## Notas

- `en_revision` es lo que se revisa. `en_espera` espera confirmación del usuario
  (`ea trabajo confirmar`): no lo toques.
- Al reformular, el trabajo vuelve a la cola del ejecutor, y se tomará con el
  ejecutor vigente en ese momento (puede haber cambiado). No esperes en bucle:
  informa y termina.
