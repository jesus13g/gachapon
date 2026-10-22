# Flujo · obra

Procedimiento neutral del **planificador** para lanzar una obra: un plan
dividido en fases pequeñas, cada una con instrucciones, entregable y criterio de
comando, que el ejecutor implementa fase a fase con revisión cada N fases.

## Reglas

- El planificador **formaliza el plan y encola**; no implementa las fases.
- **Ningún agente se confirma a sí mismo.** El criterio de cada fase lo ejecuta
  el sistema, y un criterio vacío no es éxito.
- Consulta `ea agentes` para saber qué ejecutor y qué modelo van a trabajar:
  con un modelo local pequeño, cada fase tarda minutos y debe ser muy concreta.

## Pasos

1. **Worktree.** Ruta absoluta de un repo git
   (`git -C <ruta> rev-parse --show-toplevel`, normalmente el directorio
   actual). Si `git status --porcelain` no está vacío, avisa: los cambios sin
   commit se mezclarán con el diff de la obra; sugiere hacer commit antes.
2. **Divide en fases pequeñas** (una idea por fase, 1-3 ficheros). Para cada una:
   - `numero` (1, 2, …) y `titulo`.
   - `instrucciones`: ficheros y funciones nombrados explícitamente (rutas,
     firmas, nombres exactos, casos límite). El ejecutor no infiere lo que no se
     dice.
   - `entregable`: qué existe al terminar (p. ej. «función `parse(linea)` en
     `src/parser.py`»).
   - `criterio`: comando de shell que pasa solo si la fase está hecha
     (`python -m pytest tests/test_parser.py`, `test -f …`, `grep -q …`). Las
     fases posteriores pueden reutilizar tests de las anteriores.
   - `skills` (opcional): si la fase necesita skills del ejecutor distintas de
     las de la orden; si no, hereda las de la orden.
   - Si una fase necesita decisiones de diseño, resuélvelas tú en las
     instrucciones (eso sí es formalizar).
3. **`revisar_cada`.** Obra menor (1-3 fases): `null`, revisas al terminar. Obra
   mayor: si el usuario no lo dijo, **pregúntaselo** (N fases entre revisiones;
   sugiere 2-3). Es decisión suya.
4. **Orden JSON** en un temporal (`mktemp`; `id` y `creada_en` vacíos):

```json
{
  "id": "",
  "modo": "obra",
  "titulo": "título corto",
  "worktree": "/ruta/absoluta",
  "objetivo": "contexto general de la obra, en pocas líneas",
  "skills": ["python"],
  "fases": [
    {"numero": 1, "titulo": "…", "instrucciones": "…", "entregable": "…", "criterio": "python -m pytest tests/test_a.py"},
    {"numero": 2, "titulo": "…", "instrucciones": "…", "entregable": "…", "criterio": "test -f src/b.py && python -m pytest", "skills": ["python", "tests"]}
  ],
  "revisar_cada": 2,
  "topes": {"turnos": 30, "tiempo_s": 900, "llamadas_herramienta": 60, "iteraciones": 5},
  "creada_en": "",
  "creada_por": "planificador"
}
```

5. `ea trabajo crear --orden <fichero temporal>.json` (en el directorio temporal del sistema) imprime el **id**. Infórmalo y di que las
   revisiones intermedias y la final llegan a la cola del planificador
   (`ea cola planificador`; revisión en `agentes/flujo/revisar.md`).

## Skills del ejecutor

Las skills del ejecutor se eligen con la misma llave y las mismas reglas que en
`agentes/flujo/mision.md` (catálogo con `ea qwen-skills listar`, las mínimas,
`ea qwen-skills ver/probar`). Aquí se eligen **por fase**: cada fase lleva solo
las suyas, así no se gasta contexto en lo que no usa.

## Cuándo no usarla

Si el plan es de una sola idea con un criterio, usa la misión. Si una fase no se
puede escribir con criterio comprobable, parte la fase o conviértela en una
tarea tuya de diseño.
