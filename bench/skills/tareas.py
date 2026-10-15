"""Tareas del banco de skills. Cada una: ficheros de partida, objetivo y
criterio (lo que ve Qwen), skills que elegiría Claude Code y `comprobar`, un
comprobador oculto más estricto que el criterio (incluye la trampa que la
skill debería evitar)."""

import random

random.seed(7)

# --- explorar: repo con ruido y un fichero grande -------------------------
_catalogo = "CATALOGO = [\n" + "".join(
    f"    {{'ref': 'P{i:04d}', 'nombre': 'producto {i}', 'precio': {random.randint(1, 900)}.0, 'iva_incluido': {i % 3 == 0}}},\n"
    for i in range(2500)
) + "]\n"
_modulos = {
    f"app/{d}/{m}.py": f'"""Módulo {m}."""\n\n\ndef {m}_info():\n    return "{d}.{m}"\n'
    for d, ms in {"clientes": ["alta", "baja", "buscar"], "envios": ["rutas", "tarifas"], "web": ["vistas", "formularios"]}.items()
    for m in ms
}

# --- resumir: log largo ---------------------------------------------------
_comp = ["db", "http", "cache", "cola"]
_lineas, _err, _warn, _primero, _por = [], 0, 0, None, {c: 0 for c in _comp}
for i in range(3000):
    c = random.choice(_comp)
    r = random.random()
    hora = f"2026-10-14T{8 + i // 400:02d}:{(i // 7) % 60:02d}:{i % 60:02d}"
    if r < 0.02 or (c == "cola" and r < 0.05):
        _err += 1
        _por[c] += 1
        linea = f"{hora} ERROR [{c}] fallo al procesar la petición {i}"
        _primero = _primero or linea
    elif r < 0.08:
        _warn += 1
        linea = f"{hora} WARN [{c}] latencia alta ({random.randint(200, 900)} ms)"
    else:
        linea = f"{hora} INFO [{c}] petición {i} atendida"
    _lineas.append(linea)
_LOG = "\n".join(_lineas) + "\n"
_PEOR = max(_por, key=_por.get)

TAREAS = {
    "python": {
        "skills": ["python", "tests"],
        "titulo": "Mediana",
        "objetivo": "Añade a `stats.py` la función `mediana(valores)` que devuelve la mediana de una lista de números "
        "(con un número par de elementos, la media de los dos centrales). Con la lista vacía lanza `ValueError`. "
        "Añade sus tests a `test_stats.py`.",
        "criterio": "python3 -m unittest -q test_stats",
        "ficheros": {
            "stats.py": '"""Estadística básica."""\n\n\ndef media(valores):\n    if not valores:\n        raise ValueError("lista vacía")\n    return sum(valores) / len(valores)\n',
            "test_stats.py": "import unittest\n\nfrom stats import media\n\n\nclass TestMedia(unittest.TestCase):\n    def test_media(self):\n        self.assertEqual(media([1, 2, 3]), 2)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "comprobar": "python3 -m unittest -q test_stats && python3 -c \"\n"
        "from stats import mediana\n"
        "l=[3,1,2]; assert mediana(l)==2; assert l==[3,1,2], 'muta la entrada'\n"
        "assert mediana([4,1,3,2])==2.5\n"
        "try:\n    mediana([]); raise SystemExit('no lanza ValueError')\nexcept ValueError: pass\n"
        "\" && test $(grep -c 'def test' test_stats.py) -ge 3",
    },
    "depurar": {
        "skills": ["python", "depurar"],
        "titulo": "Duraciones mal calculadas",
        "objetivo": "`python3 -m unittest -q test_parser` falla. Encuentra la causa y arréglala.",
        "criterio": "python3 -m unittest -q test_parser",
        "ficheros": {
            "parser.py": 'import re\n\n\ndef parse_duracion(texto):\n    """\'1h30m\' -> 5400, \'45m\' -> 2700, \'2h\' -> 7200."""\n'
            '    t = texto.strip()\n    m = re.fullmatch(r"(?:(\\d+)h)?(?:(\\d+)m)?", t)\n    if not t or not m:\n        raise ValueError(texto)\n'
            "    h, mi = m.groups()\n    return int(h or 0) * 3600 + int(mi or 0) * 3600\n",
            "test_parser.py": "import unittest\n\nfrom parser import parse_duracion\n\n\nclass T(unittest.TestCase):\n"
            "    def test_horas(self):\n        self.assertEqual(parse_duracion('2h'), 7200)\n\n"
            "    def test_minutos(self):\n        self.assertEqual(parse_duracion('45m'), 2700)\n\n"
            "    def test_mixto(self):\n        self.assertEqual(parse_duracion('1h30m'), 5400)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "comprobar": "git diff --quiet -- test_parser.py || { echo 'test modificado'; exit 1; }; "
        "python3 -m unittest -q test_parser && python3 -c \"\n"
        "from parser import parse_duracion as p\nassert p('1h5m')==3900\nassert p(' 10m ')==600\n"
        "try:\n    p('abc'); raise SystemExit('acepta basura')\nexcept ValueError: pass\n\"",
    },
    "rust": {
        "skills": ["rust", "tests"],
        "titulo": "Capitalizar",
        "objetivo": "En `src/lib.rs` añade `pub fn capitalizar(s: &str) -> String`, que pone en mayúscula la primera letra "
        "de cada palabra y deja el resto igual, conservando los espacios tal cual (\"hola  mundo\" -> \"Hola  Mundo\"). "
        "Debe funcionar con letras no ASCII (\"ñandú\" -> \"Ñandú\"). Añade tests unitarios.",
        "criterio": "cargo test -q",
        "ficheros": {
            "Cargo.toml": '[package]\nname = "texto"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n\n[dependencies]\n',
            "src/lib.rs": "//! Utilidades de texto.\n\n/// Número de palabras separadas por espacios.\npub fn palabras(s: &str) -> usize {\n    s.split_whitespace().count()\n}\n\n"
            "#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn cuenta_palabras() {\n        assert_eq!(palabras(\"a b  c\"), 3);\n    }\n}\n",
            ".gitignore": "/target\n",
        },
        "comprobar": "mkdir -p tests && cat > tests/oculto.rs <<'EOF'\nuse texto::capitalizar;\n#[test]\nfn oculto() {\n"
        "    assert_eq!(capitalizar(\"hola  mundo\"), \"Hola  Mundo\");\n    assert_eq!(capitalizar(\"ñandú éxito\"), \"Ñandú Éxito\");\n"
        "    assert_eq!(capitalizar(\"\"), \"\");\n    assert_eq!(capitalizar(\" a\"), \" A\");\n    assert_eq!(capitalizar(\"aBC\"), \"ABC\");\n}\nEOF\n"
        "cargo test -q 2>&1 | tail -5 && cargo test -q >/dev/null 2>&1 && test $(grep -c '#\\[test\\]' src/lib.rs) -ge 2",
    },
    "explorar": {
        "skills": ["python", "explorar"],
        "titulo": "IVA incorrecto",
        "objetivo": "Las facturas calculan mal el IVA: debe ser el 21 %. Encuentra dónde se calcula y arréglalo.",
        "criterio": "python3 -m unittest -q",
        "ficheros": {
            **_modulos,
            "app/__init__.py": "",
            "app/datos/__init__.py": "",
            "app/datos/catalogo.py": _catalogo,
            "app/fiscal/__init__.py": "",
            "app/fiscal/impuestos.py": '"""Impuestos."""\n\nTIPO_GENERAL = 0.12\n\n\ndef con_iva(base):\n    return round(base * (1 + TIPO_GENERAL), 2)\n',
            "app/facturas.py": "from app.fiscal.impuestos import con_iva\n\n\ndef total_factura(lineas):\n    return con_iva(sum(p * c for p, c in lineas))\n",
            "test_facturas.py": "import unittest\n\nfrom app.facturas import total_factura\n\n\nclass T(unittest.TestCase):\n"
            "    def test_total(self):\n        self.assertEqual(total_factura([(50.0, 2)]), 121.0)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
        },
        "comprobar": "git diff --quiet -- app/datos test_facturas.py || { echo 'tocó lo que no debía'; exit 1; }; python3 -m unittest -q",
    },
    "shell": {
        "skills": ["shell"],
        "titulo": "Contar por extensión",
        "objetivo": "Crea `contar.sh DIR`: recorre DIR recursivamente y escribe una línea `extension N` por cada extensión "
        "(sin el punto), ordenadas alfabéticamente; los ficheros sin extensión cuentan como `(sin)`. Sin argumento o si DIR "
        "no existe, muestra el uso por stderr y sale con código 2. Debe soportar nombres con espacios.",
        "criterio": "./contar.sh muestra | grep -qx 'txt 2'",
        "ficheros": {"muestra/a.txt": "a\n", "muestra/sub/b.txt": "b\n", "muestra/c.md": "c\n", "muestra/LEEME": "x\n"},
        "comprobar": "d=$(mktemp -d) && mkdir -p \"$d/con espacio\" && touch \"$d/con espacio/x y.py\" \"$d/z.py\" \"$d/Makefile\" \"$d/r.tar.gz\" "
        "&& out=$(./contar.sh \"$d\") && printf '%s\\n' \"$out\" && { [ \"$out\" = \"$(printf '(sin) 1\\ngz 1\\npy 2')\" ] || [ \"$out\" = \"$(printf 'gz 1\\npy 2\\n(sin) 1')\" ]; } "
        "&& { ./contar.sh >/dev/null 2>&1; [ $? -eq 2 ]; } && { ./contar.sh /no/existe >/dev/null 2>&1; [ $? -eq 2 ]; } "
        "&& [ -z \"$(./contar.sh 2>/dev/null)\" ]",
    },
    "refactor": {
        "skills": ["python", "refactor"],
        "titulo": "Renombrar calc_total",
        "objetivo": "Renombra la función `calc_total` a `calcular_total` en todo el proyecto, sin cambiar su comportamiento.",
        "criterio": "python3 -m unittest -q",
        "ficheros": {
            "ventas.py": "def calc_total(precios, descuento=0):\n    \"\"\"Suma precios y aplica un descuento en tanto por uno.\"\"\"\n    return round(sum(precios) * (1 - descuento), 2)\n",
            "informes.py": "from ventas import calc_total\n\n\ndef informe(pedidos):\n    return {k: calc_total(v) for k, v in pedidos.items()}\n",
            "tienda.py": "import ventas\n\n\ndef cobrar(carrito):\n    return ventas.calc_total(carrito, descuento=0.1)\n",
            "test_ventas.py": "import unittest\n\nfrom ventas import calc_total\nfrom tienda import cobrar\n\n\nclass T(unittest.TestCase):\n"
            "    def test_total(self):\n        self.assertEqual(calc_total([1, 2]), 3)\n\n    def test_cobrar(self):\n        self.assertEqual(cobrar([10, 10]), 18)\n\n\nif __name__ == '__main__':\n    unittest.main()\n",
            "README.md": "# Tienda\n\nEl total se calcula con `calc_total(precios, descuento)`.\n",
        },
        "comprobar": "! grep -rn calc_total --exclude-dir=.git . && python3 -m unittest -q && python3 -c \"\n"
        "from ventas import calcular_total as c; from informes import informe\nassert c([10,10],0.5)==10\nassert informe({'a':[1,2]})=={'a':3}\"",
    },
    "resumir": {
        "skills": ["resumir"],
        "titulo": "Resumen del log",
        "objetivo": "Lee `servidor.log` y escribe `resumen.md` con: el número total de líneas ERROR, el número de líneas WARN, "
        "la primera línea de ERROR completa y el componente (entre corchetes) con más errores.",
        "criterio": "test -s resumen.md",
        "ficheros": {"servidor.log": _LOG},
        "comprobar": f"cat resumen.md; grep -q '\\b{_err}\\b' resumen.md && grep -q '\\b{_warn}\\b' resumen.md "
        f"&& grep -qF '{_primero}' resumen.md && grep -q '{_PEOR}' resumen.md",
    },
    "documentar": {
        "skills": ["documentar"],
        "titulo": "Documentar la CLI",
        "objetivo": "Documenta en `README.md`, en una sección `## Uso`, todas las opciones de `cli.py` con un ejemplo de uso.",
        "criterio": "grep -q '## Uso' README.md",
        "ficheros": {
            "cli.py": "import argparse\n\n\ndef main():\n    p = argparse.ArgumentParser(description='Exporta registros.')\n"
            "    p.add_argument('--entrada', required=True, help='fichero de entrada')\n"
            "    p.add_argument('--formato', choices=['csv', 'json'], default='csv')\n"
            "    p.add_argument('--limite', type=int, default=100, help='máximo de registros')\n"
            "    p.add_argument('--dry-run', action='store_true', help='no escribe nada')\n"
            "    a = p.parse_args()\n    print(a)\n\n\nif __name__ == '__main__':\n    main()\n",
            "README.md": "# Exportador\n\nExporta registros a CSV o JSON.\n\n## Licencia\n\nMIT. Ver LICENSE.\n",
        },
        "comprobar": "for o in --entrada --formato --limite --dry-run csv json 100; do grep -q -- \"$o\" README.md || { echo \"falta $o\"; exit 1; }; done; "
        "for o in --salida --verbose --output -v; do grep -q -- \"$o \" README.md && { echo \"inventa $o\"; exit 1; }; done; "
        "grep -q 'MIT. Ver LICENSE.' README.md && grep -q 'Exporta registros a CSV o JSON.' README.md",
    },
}
