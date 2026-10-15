# bench/skills

Banco de las skills de Qwen (`qwen/skills/`). Cada tarea se ejecuta con y sin
sus skills, con los mismos flags que el ejecutor (bwrap, `--bare`, topes) y una
sola iteración. Un comprobador oculto, más estricto que el criterio que ve Qwen,
decide si pasa; además se miden turnos, llamadas, tokens de entrada y tiempo.

```bash
python3 bench/skills/probar.py                          # 8 tareas × con/sin
python3 bench/skills/probar.py -t shell -v con --rondas 3
python3 bench/skills/tabla.py bench/skills/resultados/<dir>
python3 bench/skills/traza.py bench/skills/resultados/<dir>/<tarea>-con-1/salida.json
```

Las tareas están en `tareas.py`; cada una lleva la trampa que su skill debería
evitar (modificar el test, mutar la entrada, dejarse el README al renombrar,
leer entero un log de 3000 líneas, inventar opciones…). Resultados en
`resultados/` (ignorado por git).

## Lo aprendido (2026-10-15, Qwen3-Coder-30B, contexto 32k)

- Cada turno reenvía el system prompt (~6-7k tokens): el coste lo marcan los
  turnos, no la longitud de la skill.
- Con `--bare` Qwen no tiene herramienta para crear ficheros; sin decírselo
  perdía 3-4 turnos por fichero nuevo. Lo dice ahora una regla fija del prompt.
  Quitar `get_goal`, `update_goal` y `notebook_edit` ahorra ~900 tokens por turno.
- Qwen sigue mejor pasos numerados y comandos concretos que pautas abstractas,
  y prefiere `read_file`/`edit` a recetas de shell: «no uses read_file con el
  fichero grande» funciona; «usa este sed» no.
- Una skill sin efecto medible se recorta: también se paga en cada turno.

Pasada final (2 rondas; refactor y rust, 3 rondas con la versión final):

| tarea | sin skills | con skills |
|---|---|---|
| shell | 0/2 · 54k tok | 2/2 · 69k |
| refactor | 0/2 · 73k | 3/3 · 104k |
| resumir | 2/2 · 259k · 287 s | 2/2 · 60k · 49 s |
| rust | 2/2 · 42k | 3/3 · 29k |
| explorar | 2/2 · 46k (129k en el peor caso) | 2/2 · 61k (estable) |
| python, depurar, documentar | 2/2 | 2/2 · mismo coste |
