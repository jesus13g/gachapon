---
descripcion: Escribir o cambiar código Python (scripts, módulos, tests con unittest o pytest).
---
- Usa el intérprete del proyecto: si hay `.venv/`, `.venv/bin/python`; si no, `python3`.
- No instales paquetes (`pip install` no tiene red). Usa la biblioteca estándar o lo que ya esté importado en el repo.
- Antes de crear un fichero, mira cómo están organizados los existentes e imítalos (imports, nombres, tipado, docstrings).
- Para comprobar sintaxis rápido: `python3 -m py_compile fichero.py`.
- Tests: si el repo usa pytest (`conftest.py`, `pytest.ini`), `python3 -m pytest -q ruta`; si no, `python3 -m unittest -v`.
- Funciones pequeñas con nombres claros; nada de variables globales nuevas.
- Rutas con `pathlib.Path`, lectura de ficheros con `encoding="utf-8"`.
