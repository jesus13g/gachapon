# bench

Banco de pruebas fuera del sistema: lanza el mismo prompt con Claude Code solo y
con el flujo ea completo (`/mision` → Qwen → `/revisar`) y compara tiempos y
costes. Ningún binario del ecosistema lo usa.

```bash
cargo build --release   # el carril ea usa target/release/{ea,ea-orchestrator}
python3 bench/comparar.py "añade suma(a, b) a calc.py con un test en test_calc.py" \
  --criterio "python test_calc.py" --watios 350
```

Cada ejecución deja en `bench/resultados/<fecha>/` (ignorado por git):

- `informe.md` / `informe.json`: resumen, fases del carril ea, cada llamada a
  Claude y cada iteración de Qwen.
- `claude/wt`, `ea/wt`: los dos worktrees, para mirar el resultado.
- `*.jsonl`: transcripción stream-json de cada llamada a Claude
  (`claude --resume <id>` desde su worktree para reabrirla).
- `ea/qwen/it-N.json`: salida de cada iteración de Qwen, con sus tiempos.
- `ea/orquestador.log`, `ea/estado/`, `ea/ecosistema.db`: el bus aislado.

Opciones útiles: `--carriles ea` (solo uno), `--secuencial`, `--modelo-claude`,
`--rondas`, `--ref`, `--repo` (otro repo). Sin `--criterio` se usa el que
formalice ea, pero si ese criterio es un test que creó ea, en el worktree de
Claude no existe: para comparar en igualdad, pásalo tú.

Limpieza: `rm -rf bench/resultados/<fecha> && git worktree prune`.
