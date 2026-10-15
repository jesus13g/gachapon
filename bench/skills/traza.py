#!/usr/bin/env python3
"""Secuencia de herramientas de una ejecución del banco: traza.py <dir>/salida.json"""
import json
import sys

for ruta in sys.argv[1:]:
    print(f"== {ruta}")
    for m in json.load(open(ruta)):
        if m.get("type") != "assistant":
            continue
        for c in m["message"]["content"]:
            if c.get("type") == "tool_use":
                arg = json.dumps(c.get("input", {}), ensure_ascii=False)
                print(f"  {c['name']:<18} {arg[:150]}")
            elif c.get("type") == "text" and c["text"].strip():
                print(f"  · {c['text'].strip()[:150]!r}")
