//! Los adaptadores de los planificadores (skills de Claude, Codex,
//! OpenCode y Antigravity) son finos: cada uno tiene que apuntar a un
//! fichero real de `agentes/flujo/` y llevar un frontmatter válido.

use std::fs;
use std::path::{Path, PathBuf};

fn raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const FLUJOS: [&str; 4] = ["mision", "obra", "rutina", "revisar"];

fn adaptadores() -> Vec<(PathBuf, &'static str)> {
    let r = raiz();
    let mut v = Vec::new();
    for f in FLUJOS {
        v.push((r.join(format!("claude/skills/{f}/SKILL.md")), f));
        v.push((r.join(format!("agentes/codex/skills/ea-{f}/SKILL.md")), f));
        v.push((r.join(format!("agentes/antigravity/skills/ea-{f}/SKILL.md")), f));
        v.push((r.join(format!("agentes/opencode/commands/ea-{f}.md")), f));
    }
    v
}

#[test]
fn el_flujo_neutral_existe_y_no_nombra_al_agente() {
    for f in FLUJOS {
        let ruta = raiz().join(format!("agentes/flujo/{f}.md"));
        let texto = fs::read_to_string(&ruta).unwrap_or_else(|_| panic!("falta {ruta:?}"));
        for prohibido in ["Claude", "Qwen3", "30B", "Codex", "OpenCode", "Antigravity"] {
            assert!(!texto.contains(prohibido), "{f}.md nombra «{prohibido}»");
        }
        assert!(texto.contains("ea cola planificador") || f != "revisar");
    }
}

#[test]
fn cada_adaptador_apunta_a_un_flujo_que_existe() {
    for (ruta, flujo) in adaptadores() {
        let texto = fs::read_to_string(&ruta).unwrap_or_else(|_| panic!("falta {ruta:?}"));
        let enlace = format!("agentes/flujo/{flujo}.md");
        assert!(texto.contains(&enlace), "{ruta:?} no remite a {enlace}");
        assert!(raiz().join(&enlace).is_file());
        // Frontmatter con descripción entre comillas (los dos puntos sin
        // comillas rompen el YAML en algunos agentes).
        assert!(texto.starts_with("---\n"), "{ruta:?} sin frontmatter");
        let cabecera = texto[4..].split("\n---\n").next().unwrap();
        let desc = cabecera
            .lines()
            .find_map(|l| l.strip_prefix("description: "))
            .unwrap_or_else(|| panic!("{ruta:?} sin description"));
        assert!(desc.starts_with('"') && desc.ends_with('"'), "{ruta:?}: description sin comillas");
    }
}
