#!/usr/bin/env python3
"""Banco de las skills de Qwen: cada tarea se ejecuta con y sin sus skills,
con los mismos flags que el ejecutor (bwrap, --bare, topes) y UNA iteración,
y se mide si pasa el comprobador oculto, turnos, llamadas a herramientas,
tokens y tiempo.

    python3 bench/skills/probar.py                 # todas, con y sin skills
    python3 bench/skills/probar.py -t depurar -v con --rondas 2

Resultados en bench/skills/resultados/<fecha>/ (ignorado por git).
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from tareas import TAREAS  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
SKILLS = REPO / "qwen" / "skills"
BASE_URL = "http://127.0.0.1:8080/v1"
MODELO = "qwen3-coder-30b"
TOPES = {"turnos": 30, "tiempo_s": 600, "llamadas": 60}


def cuerpo_skill(nombre: str) -> str:
    t = (SKILLS / f"{nombre}.md").read_text()
    if t.startswith("---\n") and "\n---\n" in t[4:]:
        t = t[4:].split("\n---\n", 1)[1]
    return t.strip()


def bloque(skills):
    """Igual que ea_bus::bloque_prompt."""
    if not skills:
        return ""
    t = "# Skills para esta tarea\nSigue estas pautas además de las instrucciones de la tarea.\n"
    for s in skills:
        t += f"\n## {s}\n{cuerpo_skill(s)}\n"
    return t


def prompt(titulo, objetivo, criterio):
    """Igual que construir_prompt del ejecutor."""
    return (
        f"# {titulo}\n\n{objetivo}\n"
        "\n## Reglas\n"
        "- Trabaja SOLO dentro del directorio actual (el worktree); no toques nada fuera.\n"
        "- No hagas commits.\n"
        "- Para crear un fichero nuevo usa run_shell_command con `cat > ruta <<'EOF'`: no hay herramienta para escribir ficheros. `edit` solo cambia ficheros que ya has leído con read_file.\n"
        f"- El criterio de éxito es el comando `{criterio}` y debe pasar.\n"
        "- Cuando termines, explica en 3 líneas qué hiciste.\n"
    )


def preparar(tarea, wt: Path):
    wt.mkdir(parents=True)
    for ruta, contenido in tarea["ficheros"].items():
        p = wt / ruta
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(contenido)
        if ruta.endswith(".sh"):
            p.chmod(0o755)
    if "preparar" in tarea:
        subprocess.run(["sh", "-c", tarea["preparar"]], cwd=wt, check=True, capture_output=True)
    run = lambda *a: subprocess.run(["git", *a], cwd=wt, check=True, capture_output=True)
    run("init", "-q")
    run("add", "-A")
    run("-c", "user.name=b", "-c", "user.email=b@b", "commit", "-qm", "inicio")


def qwen(prompt_txt, sistema, wt: Path):
    home = os.environ["HOME"]
    args = ["bwrap", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp",
            "--bind", f"{home}/.qwen", f"{home}/.qwen", "--bind", str(wt), str(wt), "--chdir", str(wt),
            "--unshare-pid", "--die-with-parent", "qwen",
            "--exclude-tools", "web_fetch", "web_search", "get_goal", "update_goal", "notebook_edit", "--bare", "--auth-type", "openai",
            "--openai-base-url", BASE_URL, "--openai-api-key", "local", "-m", MODELO,
            "--approval-mode", "yolo"]
    if sistema:
        args += ["--append-system-prompt", sistema]
    args += ["--max-session-turns", str(TOPES["turnos"]), "--max-wall-time", f"{TOPES['tiempo_s']}s",
             "--max-tool-calls", str(TOPES["llamadas"]), "--output-format", "json", prompt_txt]
    t0 = time.time()
    p = subprocess.run(args, cwd=wt, capture_output=True, text=True, timeout=TOPES["tiempo_s"] + 120,
                       env={**os.environ, "QWEN_CODE_SUPPRESS_YOLO_WARNING": "1"})
    return p.returncode, p.stdout, p.stderr, time.time() - t0


def metricas(salida: str):
    try:
        d = json.loads(salida)
    except json.JSONDecodeError:
        return {}
    r = next((x for x in d if x.get("type") == "result"), {})
    st = r.get("stats", {})
    herr = st.get("tools", {})
    tok = r.get("usage", {})
    return {
        "turnos": r.get("num_turns"),
        "llamadas": herr.get("totalCalls"),
        "fallos_herr": herr.get("totalFail"),
        "por_herr": {k: v.get("count") for k, v in herr.get("byName", {}).items()},
        "tok_entrada": tok.get("input_tokens"),
        "tok_salida": tok.get("output_tokens"),
        "resumen": (r.get("result") or "")[-600:],
    }


def comprobar(tarea, wt: Path):
    # la caché .pyc puede quedarse vieja si Qwen edita y prueba en el mismo segundo
    subprocess.run(["find", ".", "-name", "__pycache__", "-prune", "-exec", "rm", "-rf", "{}", "+"], cwd=wt)
    p = subprocess.run(["sh", "-c", "exec 2>&1; " + tarea["comprobar"]], cwd=wt, capture_output=True, text=True, timeout=180)
    return p.returncode == 0, p.stdout[-1500:]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("-t", "--tareas", nargs="*", default=list(TAREAS))
    ap.add_argument("-v", "--variantes", nargs="*", default=["sin", "con"])
    ap.add_argument("--rondas", type=int, default=1)
    ap.add_argument("--salida", default=None)
    a = ap.parse_args()

    raiz = Path(a.salida or REPO / "bench" / "skills" / "resultados" / datetime.now().strftime("%Y%m%d-%H%M%S")).resolve()
    raiz.mkdir(parents=True, exist_ok=True)
    filas = []
    for nombre in a.tareas:
        t = TAREAS[nombre]
        for ronda in range(1, a.rondas + 1):
            for var in a.variantes:
                skills = t["skills"] if var == "con" else []
                dir_ = raiz / f"{nombre}-{var}-{ronda}"
                wt = dir_ / "wt"
                preparar(t, wt)
                sistema = bloque(skills)
                codigo, salida, err, dur = qwen(prompt(t["titulo"], t["objetivo"], t["criterio"]), sistema, wt)
                (dir_ / "salida.json").write_text(salida)
                (dir_ / "stderr.txt").write_text(err)
                ok, ev = comprobar(t, wt)
                fila = {"tarea": nombre, "variante": var, "ronda": ronda, "skills": skills, "pasa": ok,
                        "codigo": codigo, "segundos": round(dur, 1), **metricas(salida), "evidencia": ev}
                filas.append(fila)
                (raiz / "resultados.json").write_text(json.dumps(filas, indent=1, ensure_ascii=False))
                print(f"{nombre:<11} {var:<4} r{ronda} {'PASA' if ok else 'FALLA'} "
                      f"{fila['segundos']:>6}s turnos={fila.get('turnos')} llamadas={fila.get('llamadas')} "
                      f"tok_in={fila.get('tok_entrada')}", flush=True)
    print(f"\n{raiz}")


if __name__ == "__main__":
    main()
