---
name: obra
description: Lanza una obra al agente local (Qwen): un plan dividido en fases pequeñas, cada una con instrucciones, entregable y criterio de comando, que se implementa fase a fase con revisión de Claude Code cada N fases. Úsala cuando el usuario tenga un plan o una tarea grande que implementar. Claude Code solo formaliza.
---

# /obra

Claude Code **formaliza el plan y encola**. NO implementa las fases. El agente local es un 30B: cada fase tarda unos 2 minutos, así que debe ser pequeña y muy concreta.

## Pasos

1. **Worktree.** Ruta absoluta de un repo git (`git -C <ruta> rev-parse --show-toplevel`, normalmente el directorio actual). Si `git status --porcelain` no está vacío, avisa: los cambios sin commit se mezclarán con el diff de la obra; sugiere hacer commit antes.
2. **Divide en fases pequeñas** (una idea por fase, 1-3 ficheros). Para cada una:
   - `numero` (1, 2, …) y `titulo`.
   - `instrucciones`: ficheros y funciones nombrados explícitamente (rutas, firmas, nombres exactos, casos límite). El modelo local no infiere lo que no se dice.
   - `entregable`: qué existe al terminar (p. ej. «función `parse(linea)` en `src/parser.py`»).
   - `criterio`: comando de shell que pasa solo si la fase está hecha (`python -m pytest tests/test_parser.py`, `test -f …`, `grep -q …`). Las fases posteriores pueden reutilizar tests de las anteriores.
   - Si una fase necesita decisiones de diseño, resuélvelas tú en las instrucciones (eso sí es formalizar).
3. **`revisar_cada`.** Obra menor (1-3 fases): `null`, revisas al terminar. Obra mayor: si el usuario no lo dijo, **pregúntaselo** (N fases entre revisiones; sugiere 2-3). Es decisión suya.
4. **Orden JSON** en un temporal (`mktemp`; `id` y `creada_en` vacíos):

```json
{
  "id": "",
  "modo": "obra",
  "titulo": "título corto",
  "worktree": "/ruta/absoluta",
  "objetivo": "contexto general de la obra, en pocas líneas",
  "fases": [
    {"numero": 1, "titulo": "…", "instrucciones": "…", "entregable": "…", "criterio": "python -m pytest tests/test_a.py"},
    {"numero": 2, "titulo": "…", "instrucciones": "…", "entregable": "…", "criterio": "test -f src/b.py && python -m pytest"}
  ],
  "revisar_cada": 2,
  "topes": {"turnos": 30, "tiempo_s": 900, "llamadas_herramienta": 60, "iteraciones": 5},
  "creada_en": "",
  "creada_por": "claude-code"
}
```

5. `ea trabajo crear --orden /tmp/…json` → imprime el **id**. Infórmalo, y di que las revisiones intermedias y la final llegan a la cola `claude-code` (`/revisar`).

Si el plan es de una sola idea con un criterio, usa `/mision`. Si una fase no se puede escribir con criterio comprobable, parte la fase o conviértela en una tarea tuya de diseño.
