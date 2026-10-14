---
name: mision
description: Lanza una misión al agente local (Qwen) para alcanzar un objetivo iterando hasta que un comando de shell pase. Úsala cuando el usuario pida «una misión» o un trabajo que se pueda describir en pasos verificables (crear/arreglar código, buscar, leer, resumir). Claude Code solo formaliza; no lo implementa.
---

# /mision

Claude Code **formaliza y encola**. NO implementa el trabajo, ni siquiera "para ir más rápido": si se puede describir como pasos verificables, lo hace el agente local (Qwen3-Coder 30B, unos 30-120 s por paso sencillo).

## Cuándo NO usarla
- Requiere diseño, arquitectura o depuración difícil: hazlo tú.
- Es demasiado grande para un 30B (muchos ficheros, decisiones abiertas): sugiere `/obra` con fases pequeñas.

## Pasos

1. **Worktree.** Ruta absoluta de un repo git (por defecto el directorio actual; comprueba `git -C <ruta> rev-parse --show-toplevel`). Si `git status --porcelain` muestra cambios sin commit, avisa al usuario (el diff final los mezclaría).
2. **Formaliza** y enséñale al usuario en pocas líneas:
   - `objetivo`: texto concreto para el agente: ficheros y funciones nombrados, firmas, comportamiento esperado. Sin ambigüedad.
   - `criterio_exito`: UN comando de shell que pase o falle, ejecutado desde el worktree: tests (`python test_x.py`, `cargo test -p x`), `test -f ruta`, `grep -q patrón fichero` o un script. Debe fallar ahora y pasar solo si el objetivo se cumple. Si no existe, el objetivo incluye crear el test, pero prefiere escribir tú el test antes (es formalizar, no implementar) si es corto.
   - `topes`: `iteraciones` 3-5 (por defecto 5); `tiempo_s` 900 por iteración; `turnos` 30; `llamadas_herramienta` 60. Sube solo con motivo.
3. **Escribe la orden** en un temporal (`mktemp`) con este formato (`id`, `creada_en` vacíos; se rellenan solos):

```json
{
  "id": "",
  "modo": "mision",
  "titulo": "título corto",
  "worktree": "/ruta/absoluta",
  "objetivo": "…",
  "criterio_exito": "python test_calc.py",
  "topes": {"turnos": 30, "tiempo_s": 900, "llamadas_herramienta": 60, "iteraciones": 4},
  "creada_en": "",
  "creada_por": "claude-code"
}
```

4. **Encola:** `ea trabajo crear --orden /tmp/…json` → imprime el **id**.
5. **Informa** del id y de que llegará a la cola `claude-code` al terminar o bloquearse (`ea cola claude-code`). Revisión con `/revisar`. No esperes en bucle ni hagas el trabajo mientras tanto.

Forma rápida sin JSON: `ea trabajo crear --modo mision --titulo T --worktree . --objetivo O --criterio C` (topes por defecto).
