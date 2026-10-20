---
name: revisar
description: Revisa los trabajos que el agente local ha entregado a Claude Code (cola claude-code): misiones, obras y rutinas. Lee el resumen, abre el diff solo si hace falta y decide cerrar, reformular o continuar. Úsala cuando el usuario pida revisar, o tras lanzar una misión/obra/rutina.
---

# /revisar

Claude Code **decide**, no rehace. Cuesta cuota leer de más: el resumen debe bastar casi siempre.

## Flujo

1. `ea cola claude-code --json` → lista de entradas (`trabajo`, `motivo`). Vacía: dilo y termina.
2. Por cada trabajo: `ea trabajo ver <id>` (modo, estado, iteración/fase, resultado, motivo, evidencia, diff stat, `obra_completa`). Si hay muchos, trátalos uno a uno.
3. **Abre el diff solo si el resumen no basta**: `ea trabajo diff <id>` (completo, con ficheros nuevos) o `ea trabajo diff <id> --fase N` (una fase; necesita las instantáneas del store). Lee solo la parte necesaria (`| head`, `| sed -n`, o un fichero concreto).
4. **Decide**:
   - **Cerrar** (`ea trabajo cerrar <id>`): resultado `Terminado` y además correcto de verdad. Comprueba la evidencia y el acierto frente al objetivo (el diff stat toca lo esperado, la salida del criterio es real, no hay trampas como tests vaciados o criterio trivial). Que el criterio pase no basta.
   - **Reformular** (`ea trabajo reformular <id> --nota "…"`): falló, quedó `Bloqueado` o se queda corto. La nota va al agente local: instrucciones concretas (qué fichero, qué función, qué falla, qué comando probar). Nada de «hazlo mejor». En una obra se rehace la fase pendiente (en una obra completa, la última); si el fallo está en una fase anterior, añade `--fase N`: esa y las siguientes vuelven a quedar pendientes.
   - **Continuar** (`ea trabajo continuar <id>`): obra con `obra_completa: false` y resultado `Terminado` en una revisión intermedia; las fases hechas están bien, sigue con las siguientes. Si una fase hecha está mal, reformula con nota en vez de continuar.
   - **Corrección de rutina** (existe `paquete-correccion.md` en el directorio del trabajo): sigue la sección «Corrección» de la skill `rutina`.
   - **Cancelar** (`ea trabajo cancelar <id>`) solo si el usuario lo pide o el objetivo ya no tiene sentido.
5. **No rehagas el trabajo tú** salvo que lo exija criterio (diseño o depuración difícil que el 30B no puede). Si reformulaste ya dos veces sin avance, dilo al usuario y propón qué hacer.
6. **Resumen final**: una línea por trabajo: `<id> · <título> · cerrado|reformulado|continuado · motivo breve`.

## Notas
- Estados: `en_revision` es lo que se revisa; `en_espera` espera confirmación del usuario (`ea trabajo confirmar`), no lo toques.
- Al reformular, el trabajo vuelve a `modelo-local`; no esperes en bucle, informa y termina.
