---
descripcion: Escribir o cambiar código Python (scripts, módulos, tests con unittest o pytest).
---
- Intérprete: `.venv/bin/python` si existe; si no, `python3`. No instales paquetes: no hay red.
- Imita los ficheros existentes (imports, nombres, docstrings). No modifiques los argumentos que recibe una función (p. ej. usa `sorted(x)`, no `x.sort()`).
- Tests: pytest si el repo lo usa (`conftest.py`), si no `python3 -m unittest -q`.
