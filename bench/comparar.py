#!/usr/bin/env python3
"""Compara el mismo prompt en dos carriles y mide tiempos y costes.

  claude  Claude Code solo, en su propio worktree.
  ea      El flujo ea completo: Claude Code formaliza con /mision, un
          ea-orchestrator AISLADO ejecuta Qwen hasta que pasa el criterio y
          Claude Code revisa con /revisar (hasta --rondas veces).

Nada toca el sistema real: cada carril trabaja en un worktree nuevo y el
carril ea usa su propio bus, base de datos, bóveda y orquestador.toml dentro
de bench/resultados/<fecha>/. Los servicios reales (ea-orchestrator, ea-mcp,
ea-web) no ven nada. Lo único compartido es llama-server.

Uso:
  python3 bench/comparar.py "añade una función suma a calc.py con su test"
  python3 bench/comparar.py -f prompt.txt --repo ~/otro-repo --criterio "pytest -q"
"""

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
from datetime import datetime
from pathlib import Path

RAIZ = Path(__file__).resolve().parent.parent
BIN = RAIZ / "target" / "release"
MCP_VACIO = '{"mcpServers":{}}'
NO_INTERACTIVO = (
    "Estás en modo no interactivo (prueba de rendimiento): no pidas confirmación "
    "ni preguntes nada; decide tú y termina."
)
_imprimir = threading.Lock()


def log(carril, msg):
    with _imprimir:
        print(f"[{datetime.now():%H:%M:%S}] [{carril}] {msg}", flush=True)


def sh(args, cwd=None, check=True, **kw):
    return subprocess.run(args, cwd=cwd, check=check, text=True, capture_output=True, **kw)


# --------------------------------------------------------------------------
# Claude Code
# --------------------------------------------------------------------------

def resumen_evento(ev):
    """Una línea legible por evento de stream-json (None si no interesa)."""
    if ev.get("type") == "assistant":
        partes = []
        for b in ev.get("message", {}).get("content", []):
            if b.get("type") == "tool_use":
                entrada = b.get("input", {})
                det = entrada.get("command") or entrada.get("file_path") or entrada.get("skill") or ""
                partes.append(f"→ {b['name']} {str(det).splitlines()[0][:100] if det else ''}")
            elif b.get("type") == "text" and b.get("text", "").strip():
                partes.append("· " + b["text"].strip().splitlines()[0][:120])
        return "\n".join(partes) or None
    if ev.get("type") == "result":
        return f"fin: {ev.get('subtype')} · {ev.get('num_turns')} turnos · ${ev.get('total_cost_usd', 0):.4f}"
    return None


def correr_claude(carril, etapa, prompt, cwd, salida_dir, args, env=None, extra=()):
    """Lanza `claude -p` en stream-json, guarda el JSONL y devuelve sus métricas."""
    cmd = [
        "claude", "-p", prompt,
        "--output-format", "stream-json", "--verbose",
        "--dangerously-skip-permissions",
        "--strict-mcp-config", "--mcp-config", MCP_VACIO,
        *extra,
    ]
    if args.modelo_claude:
        cmd += ["--model", args.modelo_claude]
    fichero = salida_dir / f"{etapa}.jsonl"
    log(carril, f"{etapa}: claude -p (transcripción en {fichero.relative_to(args.dir)})")
    ini = time.time()
    resultado = {}
    with open(fichero, "w") as f, open(salida_dir / f"{etapa}.stderr", "w") as err:
        p = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=err,
                             stdin=subprocess.DEVNULL, text=True)
        for linea in p.stdout:
            f.write(linea)
            try:
                ev = json.loads(linea)
            except json.JSONDecodeError:
                continue
            if ev.get("type") == "result":
                resultado = ev
            r = resumen_evento(ev)
            if r and not args.silencio:
                for l in r.splitlines():
                    log(carril, f"  {etapa} {l}")
        p.wait()
    fin = time.time()
    u = resultado.get("usage", {})
    return {
        "etapa": etapa,
        "codigo": p.returncode,
        "ini": ini,
        "fin": fin,
        "pared_s": round(fin - ini, 1),
        "duracion_api_s": round(resultado.get("duration_api_ms", 0) / 1000, 1),
        "turnos": resultado.get("num_turns"),
        "coste_usd": resultado.get("total_cost_usd", 0.0),
        "tokens": {
            "entrada": u.get("input_tokens", 0),
            "salida": u.get("output_tokens", 0),
            "cache_lectura": u.get("cache_read_input_tokens", 0),
            "cache_escritura": u.get("cache_creation_input_tokens", 0),
        },
        "por_modelo": resultado.get("modelUsage", {}),
        "error": resultado.get("is_error", not resultado),
        "session_id": resultado.get("session_id"),
        "cwd": str(cwd),
        "respuesta": resultado.get("result", ""),
    }


def sumar_claude(etapas):
    t = {"coste_usd": 0.0, "turnos": 0, "duracion_api_s": 0.0,
         "tokens": {"entrada": 0, "salida": 0, "cache_lectura": 0, "cache_escritura": 0}}
    for e in etapas:
        t["coste_usd"] += e["coste_usd"] or 0
        t["turnos"] += e["turnos"] or 0
        t["duracion_api_s"] += e["duracion_api_s"]
        for k in t["tokens"]:
            t["tokens"][k] += e["tokens"][k]
    return t


# --------------------------------------------------------------------------
# Worktrees y verificación
# --------------------------------------------------------------------------

def crear_worktree(repo, ref, destino):
    sh(["git", "-C", str(repo), "worktree", "add", "--detach", str(destino), ref])
    return destino


def cambios(wt):
    stat = sh(["git", "diff", "--stat", "HEAD"], cwd=wt, check=False).stdout.strip()
    nuevos = [l for l in sh(["git", "ls-files", "--others", "--exclude-standard"], cwd=wt,
                            check=False).stdout.splitlines() if "__pycache__" not in l]
    modificados = [l for l in sh(["git", "diff", "--name-only", "HEAD"], cwd=wt,
                                 check=False).stdout.splitlines()]
    return {"ficheros": len(modificados) + len(nuevos), "nuevos": nuevos, "stat": stat}


def verificar(wt, criterio, tope_s=600):
    if not criterio:
        return None
    ini = time.time()
    try:
        r = subprocess.run(["sh", "-c", criterio], cwd=wt, text=True, capture_output=True,
                           timeout=tope_s)
        pasa, salida = r.returncode == 0, (r.stdout + r.stderr)
    except subprocess.TimeoutExpired:
        pasa, salida = False, f"tiempo agotado ({tope_s}s)"
    return {"pasa": pasa, "duracion_s": round(time.time() - ini, 1), "cola_salida": salida[-1500:]}


# --------------------------------------------------------------------------
# Carril claude
# --------------------------------------------------------------------------

def carril_claude(args, res):
    d = args.dir / "claude"
    d.mkdir()
    wt = crear_worktree(args.repo, args.ref, d / "wt")
    ini = time.time()
    # Sin Skill para que no delegue en /mision por su cuenta.
    e = correr_claude("claude", "claude", args.prompt, wt, d, args,
                      extra=["--disallowedTools", "Skill"])
    res["claude"] = {"worktree": str(wt), "pared_s": round(time.time() - ini, 1),
                     "etapas": [e], "total_claude": sumar_claude([e])}
    log("claude", f"terminado en {res['claude']['pared_s']} s · ${e['coste_usd']:.4f}")


# --------------------------------------------------------------------------
# Carril ea (orquestador aislado)
# --------------------------------------------------------------------------

WRAPPER_QWEN = """#!/usr/bin/env bash
# Envoltorio de qwen para el banco: guarda cada iteración con sus tiempos.
# Corre dentro de bwrap, donde solo ~/.qwen y el worktree son escribibles.
unset XDG_CONFIG_HOME
d="$HOME/.qwen/ea-bench/{run}"
mkdir -p "$d"
n=$(( $(ls "$d"/it-*.json 2>/dev/null | wc -l) + 1 ))
ini=$(date +%s.%N)
{qwen} "$@" > "$d/it-$n.json"
rc=$?
fin=$(date +%s.%N)
printf '{{"iteracion": %d, "ini": %s, "fin": %s, "codigo": %d}}\\n' "$n" "$ini" "$fin" "$rc" > "$d/it-$n.meta"
cat "$d/it-$n.json"
exit $rc
"""


def preparar_ea(args, d):
    """Estado, base, bóveda y orquestador.toml propios; devuelve el entorno."""
    estado, config = d / "estado", d / "config" / "ecosistema-asistente"
    for p in (estado, config, d / "boveda"):
        p.mkdir(parents=True)
    qwen = shutil.which("qwen")
    if not qwen:
        sys.exit("no encuentro `qwen` en el PATH")
    wrapper = d / "qwen-bench"
    wrapper.write_text(WRAPPER_QWEN.format(run=args.dir.name, qwen=qwen))
    wrapper.chmod(0o755)
    # orquestador.toml: el real si existe, con qwen_bin y avisos sustituidos.
    real = Path.home() / ".config" / "ecosistema-asistente" / "orquestador.toml"
    lineas = real.read_text().splitlines() if real.exists() else []
    propias = [f'qwen_bin = "{wrapper}"', "avisos_escritorio = false"]
    en_raiz, resto = True, []
    for l in lineas:
        if l.strip().startswith("["):
            en_raiz = False
        if en_raiz and l.split("=")[0].strip() in ("qwen_bin", "avisos_escritorio"):
            continue
        resto.append(l)
    (config / "orquestador.toml").write_text("\n".join(propias + resto) + "\n")
    env = dict(os.environ)
    env.update({
        "EA_ESTADO_DIR": str(estado),
        "EA_DB_PATH": str(d / "ecosistema.db"),
        "EA_BOVEDA_DIR": str(d / "boveda"),
        "PATH": f"{BIN}:{env['PATH']}",
    })
    return env, estado


def estados(estado_dir):
    """{id: estado} de todos los trabajos del bus aislado."""
    out = {}
    for f in (estado_dir / "trabajos").glob("*/estado.json"):
        try:
            out[f.parent.name] = json.loads(f.read_text())
        except (OSError, json.JSONDecodeError):
            pass
    return out


def cola(estado_dir, agente):
    return sorted((estado_dir / "colas" / agente).glob("*.json"))


def esperar_qwen(estado_dir, args, linea_tiempo, vistos):
    """Espera a que no quede nada en cola ni ejecutando; anota las transiciones."""
    limite = time.time() + args.tope_min * 60
    while time.time() < limite:
        ahora = estados(estado_dir)
        for i, e in ahora.items():
            clave = (e.get("estado"), e.get("iteracion"))
            if vistos.get(i) != clave:
                vistos[i] = clave
                linea_tiempo.append({"t": time.time(), "trabajo": i, **e})
                log("ea", f"  trabajo {i}: {e.get('estado')} (iteración {e.get('iteracion')})")
        ocupado = any(e.get("estado") in ("en_cola", "ejecutando") for e in ahora.values())
        if not ocupado and not cola(estado_dir, "modelo-local"):
            return True
        time.sleep(2)
    log("ea", f"tope de {args.tope_min} min esperando a Qwen")
    return False


def metricas_qwen(dir_iter):
    """Lee las iteraciones que guardó el envoltorio."""
    its = []
    for meta in sorted(dir_iter.glob("it-*.meta"), key=lambda p: int(p.stem.split("-")[1])):
        m = json.loads(meta.read_text())
        datos = meta.with_suffix(".json")
        res, usos = {}, []
        try:
            msgs = json.loads(datos.read_text())
            for x in msgs:
                if x.get("type") == "result":
                    res = x
                elif x.get("type") == "assistant":
                    usos.append(x.get("message", {}).get("usage") or {})
        except (OSError, json.JSONDecodeError, AttributeError):
            pass
        u = res.get("usage") or {}
        if not u and usos:  # sin usage en el result: suma por mensaje
            u = {k: sum(x.get(k, 0) or 0 for x in usos) for k in ("input_tokens", "output_tokens")}
        its.append({
            "iteracion": m["iteracion"],
            "codigo": m["codigo"],
            "pared_s": round(m["fin"] - m["ini"], 1),
            "turnos": res.get("num_turns"),
            "tokens": {
                "entrada": u.get("input_tokens", 0) or 0,
                "salida": u.get("output_tokens", 0) or 0,
                "cache_lectura": u.get("cache_read_input_tokens", 0) or 0,
            },
            "ini": m["ini"],
            "fin": m["fin"],
        })
    return its


def carril_ea(args, res):
    d = args.dir / "ea"
    d.mkdir()
    wt = crear_worktree(args.repo, args.ref, d / "wt")
    env, estado = preparar_ea(args, d)
    env_orq = dict(env, XDG_CONFIG_HOME=str(d / "config"), RUST_LOG="info")
    log_orq = open(d / "orquestador.log", "w")
    orq = subprocess.Popen([str(BIN / "ea-orchestrator")], env=env_orq, stdout=log_orq,
                           stderr=subprocess.STDOUT, start_new_session=True)
    res["ea"] = {"worktree": str(wt), "etapas": [], "linea_tiempo": [], "trabajos": {}}
    r = res["ea"]
    ini = time.time()
    vistos = {}
    try:
        prompt = (f"/mision {args.prompt}\n\nWorktree: {wt}\n\n{NO_INTERACTIVO} "
                  "No esperes al agente local: encola y termina.")
        r["etapas"].append(correr_claude("ea", "formalizar", prompt, wt, d, args, env=env))
        if not estados(estado):
            log("ea", "la formalización no creó ningún trabajo (¿lo hizo Claude él mismo?)")
        for ronda in range(1, args.rondas + 1):
            if not esperar_qwen(estado, args, r["linea_tiempo"], vistos):
                break
            if not cola(estado, "claude-code"):
                break
            prompt = f"/revisar\n\n{NO_INTERACTIVO}"
            r["etapas"].append(correr_claude("ea", f"revisar-{ronda}", prompt, wt, d, args, env=env))
        else:
            esperar_qwen(estado, args, r["linea_tiempo"], vistos)
        if cola(estado, "claude-code"):
            log("ea", f"quedan trabajos sin revisar tras {args.rondas} rondas")
    finally:
        try:
            os.killpg(orq.pid, signal.SIGTERM)
            orq.wait(timeout=15)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            os.killpg(orq.pid, signal.SIGKILL)
        log_orq.close()
    r["pared_s"] = round(time.time() - ini, 1)
    # Iteraciones de Qwen: del envoltorio (en ~/.qwen) al directorio del banco.
    origen = Path.home() / ".qwen" / "ea-bench" / args.dir.name
    if origen.exists():
        shutil.move(str(origen), d / "qwen")
    r["qwen"] = metricas_qwen(d / "qwen") if (d / "qwen").exists() else []
    for i in estados(estado):
        t = estado / "trabajos" / i
        r["trabajos"][i] = {
            n: json.loads((t / f).read_text())
            for n, f in (("orden", "orden.json"), ("estado", "estado.json"), ("informe", "informe.json"))
            if (t / f).exists()
        }
    r["total_claude"] = sumar_claude(r["etapas"])
    qt = r["qwen"]
    r["total_qwen"] = {
        "iteraciones": len(qt),
        "pared_s": round(sum(i["pared_s"] for i in qt), 1),
        "tokens": {k: sum(i["tokens"][k] for i in qt) for k in ("entrada", "salida", "cache_lectura")},
    }
    if args.watios:
        kwh = args.watios * r["total_qwen"]["pared_s"] / 3600 / 1000
        r["total_qwen"]["electricidad_eur"] = round(kwh * args.precio_kwh, 5)
    log("ea", f"terminado en {r['pared_s']} s · ${r['total_claude']['coste_usd']:.4f} de Claude · "
              f"{len(qt)} iteraciones de Qwen ({r['total_qwen']['pared_s']} s)")


# --------------------------------------------------------------------------
# Informe
# --------------------------------------------------------------------------

def fmt_tok(t):
    return f"{t['entrada']:,} / {t['salida']:,} / {t['cache_lectura']:,}".replace(",", ".")


def fases_ea(r):
    """Desglose de la pared del carril ea en fases contiguas."""
    if not r.get("etapas"):
        return []
    filas, ini = [], r["etapas"][0]["ini"]
    for e in r["etapas"]:
        filas.append((e["etapa"], e["ini"], e["fin"]))
    for q in r.get("qwen", []):
        filas.append((f"qwen it-{q['iteracion']}", q["ini"], q["fin"]))
    filas.sort(key=lambda f: f[1])
    out, cursor = [], ini
    for nombre, a, b in filas:
        if a - cursor > 1:
            out.append(("espera / criterio / sondeo", round(a - cursor, 1)))
        out.append((nombre, round(b - a, 1)))
        cursor = max(cursor, b)
    return out


def informe(args, res):
    c, e = res.get("claude"), res.get("ea")
    L = [f"# Comparativa {args.dir.name}", "", "## Prompt", "", "```", args.prompt, "```", ""]
    if res.get("criterio"):
        L += [f"Criterio de verificación ({res['criterio_origen']}): `{res['criterio']}`", ""]
    L += ["## Resumen", "", "| | Claude Code | ea (Claude + Qwen) |", "|---|---|---|"]

    def col(r, f):
        try:
            return f(r) if r else "—"
        except (KeyError, TypeError):  # carril que se rompió a medias
            return "—"

    filas = [
        ("Tiempo total (pared)", lambda r: f"{r['pared_s']} s"),
        ("Coste Claude", lambda r: f"${r['total_claude']['coste_usd']:.4f}"),
        ("Llamadas a Claude", lambda r: str(len(r["etapas"]))),
        ("Turnos Claude", lambda r: str(r["total_claude"]["turnos"])),
        ("Tiempo API Claude", lambda r: f"{r['total_claude']['duracion_api_s']} s"),
        ("Tokens Claude (ent / sal / caché)", lambda r: fmt_tok(r["total_claude"]["tokens"])),
        ("Iteraciones Qwen", lambda r: str(r["total_qwen"]["iteraciones"]) if "total_qwen" in r else "—"),
        ("Tiempo Qwen", lambda r: f"{r['total_qwen']['pared_s']} s" if "total_qwen" in r else "—"),
        ("Tokens Qwen (ent / sal / caché)", lambda r: fmt_tok(r["total_qwen"]["tokens"]) if "total_qwen" in r else "—"),
        ("Electricidad Qwen", lambda r: f"{r['total_qwen']['electricidad_eur']} €"
            if r.get("total_qwen", {}).get("electricidad_eur") is not None else "—"),
        ("Ficheros cambiados", lambda r: str(r["cambios"]["ficheros"])),
        ("Criterio", lambda r: ("pasa" if r["verificacion"]["pasa"] else "falla")
            if r.get("verificacion") else "—"),
    ]
    for nombre, f in filas:
        L.append(f"| {nombre} | {col(c, f)} | {col(e, f)} |")
    L.append("")
    if e:
        L += ["## Fases del carril ea", "", "| Fase | s |", "|---|---|"]
        L += [f"| {n} | {s} |" for n, s in fases_ea(e)]
        L.append("")
        for i, t in e.get("trabajos", {}).items():
            inf = t.get("informe", {})
            L.append(f"- `{i}` · {t.get('orden', {}).get('titulo', '')} · "
                     f"{t.get('estado', {}).get('estado')} · {inf.get('resultado', '')} "
                     f"{inf.get('motivo') or ''}")
        L.append("")
    L += ["## Por llamada a Claude", "", "| Carril | Etapa | Pared s | API s | Turnos | USD | Tokens (ent / sal / caché) |",
          "|---|---|---|---|---|---|---|"]
    for nombre, r in (("claude", c), ("ea", e)):
        for x in (r or {}).get("etapas", []):
            L.append(f"| {nombre} | {x['etapa']} | {x['pared_s']} | {x['duracion_api_s']} | {x['turnos']} | "
                     f"{x['coste_usd']:.4f} | {fmt_tok(x['tokens'])} |")
    L.append("")
    if e and e.get("qwen"):
        L += ["## Iteraciones de Qwen", "", "| It | Pared s | Turnos | Código | Tokens (ent / sal / caché) |",
              "|---|---|---|---|---|"]
        L += [f"| {q['iteracion']} | {q['pared_s']} | {q['turnos']} | {q['codigo']} | {fmt_tok(q['tokens'])} |"
              for q in e["qwen"]]
        L.append("")
    L += ["## Depurar", ""]
    for nombre, r in (("claude", c), ("ea", e)):
        for x in (r or {}).get("etapas", []):
            if x.get("session_id"):
                L.append(f"- {nombre}/{x['etapa']}: `cd {x['cwd']} && claude --resume {x['session_id']}`")
    L += ["", f"Transcripciones, salidas de Qwen y log del orquestador en `{args.dir}`."]
    return "\n".join(L) + "\n"


# --------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("prompt", nargs="?", help="el prompt (o usa -f)")
    ap.add_argument("-f", "--prompt-fichero", type=Path)
    ap.add_argument("--repo", type=Path, default=Path.cwd(), help="repo git sobre el que trabajar (por defecto el actual)")
    ap.add_argument("--ref", default="HEAD", help="commit de partida de ambos worktrees")
    ap.add_argument("--criterio", help="comando que verifica ambos resultados (por defecto, el que formalice ea)")
    ap.add_argument("--carriles", default="claude,ea", help="claude, ea o claude,ea")
    ap.add_argument("--secuencial", action="store_true", help="un carril detrás de otro (por defecto en paralelo)")
    ap.add_argument("--modelo-claude", help="--model para todas las llamadas a Claude")
    ap.add_argument("--rondas", type=int, default=3, help="máximo de revisiones en el carril ea")
    ap.add_argument("--tope-min", type=int, default=60, help="espera máxima a Qwen por ronda")
    ap.add_argument("--watios", type=float, default=0, help="consumo medio del equipo con Qwen, para estimar electricidad")
    ap.add_argument("--precio-kwh", type=float, default=0.15)
    ap.add_argument("--silencio", action="store_true", help="no mostrar los eventos de Claude en vivo")
    args = ap.parse_args()

    if args.prompt_fichero:
        args.prompt = args.prompt_fichero.read_text().strip()
    if not args.prompt:
        ap.error("falta el prompt")
    args.repo = Path(sh(["git", "-C", str(args.repo), "rev-parse", "--show-toplevel"]).stdout.strip())
    args.ref = sh(["git", "-C", str(args.repo), "rev-parse", args.ref]).stdout.strip()
    carriles = [c.strip() for c in args.carriles.split(",") if c.strip()]
    if "ea" in carriles:
        for b in ("ea", "ea-orchestrator"):
            if not (BIN / b).exists():
                sys.exit(f"falta {BIN / b}: cargo build --release")
        if any((BIN / "ea-orchestrator").stat().st_mtime < p.stat().st_mtime
               for p in RAIZ.glob("crates/*/src/**/*.rs")):
            print("aviso: target/release es más antiguo que el código; considera cargo build --release")
        real = Path.home() / ".local" / "state" / "ecosistema-asistente"
        if any((real / "colas" / "modelo-local").glob("*.json")):
            print("aviso: el orquestador real tiene trabajo en cola; compartirá llama-server y los tiempos de Qwen saldrán inflados")

    args.dir = RAIZ / "bench" / "resultados" / datetime.now().strftime("%Y%m%d-%H%M%S")
    args.dir.mkdir(parents=True)
    (args.dir / "prompt.txt").write_text(args.prompt + "\n")
    print(f"resultados en {args.dir}\nrepo {args.repo} @ {args.ref[:10]}\n")

    res = {"prompt": args.prompt, "repo": str(args.repo), "ref": args.ref, "carriles": carriles}
    fns = {"claude": carril_claude, "ea": carril_ea}

    def seguro(fn):
        try:
            fn(args, res)
        except Exception as ex:  # que un carril roto no tire el otro
            log(fn.__name__, f"ERROR: {ex!r}")
            res.setdefault("errores", []).append(f"{fn.__name__}: {ex!r}")

    hilos = [threading.Thread(target=seguro, args=(fns[c],)) for c in carriles]
    for h in hilos:
        h.start()
        if args.secuencial:
            h.join()
    for h in hilos:
        h.join()

    # Verificación con el mismo comando en los dos worktrees.
    criterio, origen = args.criterio, "--criterio"
    if not criterio and res.get("ea", {}).get("trabajos"):
        primero = sorted(res["ea"]["trabajos"].values(), key=lambda t: t.get("orden", {}).get("creada_en", ""))[0]
        criterio, origen = primero.get("orden", {}).get("criterio_exito"), "formalizado por ea"
    res["criterio"], res["criterio_origen"] = criterio, origen
    for c in carriles:
        if c in res:
            wt = Path(res[c]["worktree"])
            res[c]["cambios"] = cambios(wt)
            res[c]["verificacion"] = verificar(wt, criterio)

    (args.dir / "informe.json").write_text(json.dumps(res, indent=2, ensure_ascii=False))
    md = informe(args, res)
    (args.dir / "informe.md").write_text(md)
    print("\n" + md)
    if origen == "formalizado por ea" and "claude" in res:
        print("ojo: el criterio lo escribió ea; si creó un test nuevo, en el worktree de claude no existe "
              "y fallará por eso. Para comparar en igualdad, pasa --criterio.")
    print(f"limpiar: git -C {args.repo} worktree prune tras borrar {args.dir}")


if __name__ == "__main__":
    main()
