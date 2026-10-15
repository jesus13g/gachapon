#!/usr/bin/env python3
"""Tabla resumen de una o varias pasadas: tabla.py resultados/r1 resultados/r2 …"""
import json
import sys
from collections import defaultdict
from pathlib import Path

for d in sys.argv[1:]:
    filas = json.loads((Path(d) / "resultados.json").read_text())
    g = defaultdict(list)
    for f in filas:
        g[(f["tarea"], f["variante"])].append(f)
    print(f"\n## {d}\n\n| tarea | variante | pasa | tokens entrada (media) | turnos (media) | s (media) |\n|---|---|---|---|---|---|")
    for (t, v), fs in sorted(g.items()):
        n = len(fs)
        med = lambda k: round(sum(f.get(k) or 0 for f in fs) / n)
        print(f"| {t} | {v} | {sum(f['pasa'] for f in fs)}/{n} | {med('tok_entrada'):,} | {med('turnos')} | {med('segundos')} |")
