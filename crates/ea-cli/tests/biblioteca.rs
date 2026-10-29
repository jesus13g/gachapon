//! `ea biblioteca` de extremo a extremo, con el binario real y todo el estado
//! (HOME, proyecto, ajustes, base de datos, bus y runtime) en un directorio
//! temporal: nunca el HOME del usuario. Sin terminal interactiva (stdin nulo),
//! así que activar, desactivar y abrir sesiones solo pueden proponer.

use std::fs;
use std::path::Path;
use std::process::Output;

use ea_pruebas::biblioteca::{MCP, SECRETOS, SKILL};
use ea_pruebas::sin_secretos;
use ea_store::{PropuestaFilter, Store};
use serde_json::Value;

struct Mundo {
    d: tempfile::TempDir,
}

/// El HOME pequeño de `ea_pruebas::biblioteca` (con una transcripción con un
/// uso de la skill) y un proyecto dentro.
fn mundo() -> Mundo {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    fs::create_dir_all(home.join("proyecto")).unwrap();
    ea_pruebas::biblioteca::home(&home);
    Mundo { d }
}

impl Mundo {
    fn ruta(&self, p: &str) -> std::path::PathBuf {
        self.d.path().join(p)
    }

    fn ea(&self, args: &[&str]) -> Output {
        let d = self.d.path();
        ea_pruebas::cli::ea(env!("CARGO_BIN_EXE_ea"), d).args(args).current_dir(d.join("home/proyecto")).output().expect("lanzar ea")
    }

    fn json(&self, args: &[&str]) -> Value {
        let o = self.ea(args);
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{args:?}: {e}: {}", String::from_utf8_lossy(&o.stdout)))
    }

    fn propuestas(&self) -> Vec<ea_store::Propuesta> {
        Store::open_at(&self.ruta("ea.db")).unwrap().list_propuestas(&PropuestaFilter::default()).unwrap()
    }
}

fn salida(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn listar_en_json_filtra_y_no_lleva_secretos() {
    let m = mundo();
    let v = m.json(&["biblioteca", "listar", "--json"]);
    let ids: Vec<&str> = v["elementos"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&SKILL) && ids.contains(&MCP), "{ids:?}");
    let skill = v["elementos"].as_array().unwrap().iter().find(|e| e["id"] == SKILL).unwrap();
    assert_eq!(skill["activado"], "si");
    assert_eq!(skill["uso"]["total"], 1, "{skill}");
    sin_secretos(&v.to_string(), SECRETOS);

    let v = m.json(&["biblioteca", "listar", "--json", "--tipo", "mcp", "--agente", "claude-code"]);
    let ids: Vec<&str> = v["elementos"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert!(ids.iter().all(|i| i.starts_with("claude-code/mcp/")));
    let v = m.json(&["biblioteca", "listar", "--json", "--desactivados"]);
    assert!(v["elementos"].as_array().unwrap().is_empty());

    let o = m.ea(&["biblioteca", "listar", "--tipo", "nada"]);
    assert!(!o.status.success());
    assert!(salida(&o).contains("tipo desconocido"), "{}", salida(&o));
}

#[test]
fn listar_en_texto_agrupa_por_categoria_y_tipo() {
    let m = mundo();
    let o = m.ea(&["biblioteca", "listar"]);
    assert!(o.status.success(), "{}", salida(&o));
    let t = salida(&o);
    assert!(t.contains("== usuario") && t.contains("skill (1)") && t.contains("mcp (2)"), "{t}");
    assert!(t.contains("propia") && t.contains("Cómo se cambia"), "{t}");
    sin_secretos(&t, SECRETOS);
}

#[test]
fn ver_da_la_ficha_con_el_mcp_redactado_y_el_uso() {
    let m = mundo();
    let o = m.ea(&["biblioteca", "ver", MCP]);
    assert!(o.status.success(), "{}", salida(&o));
    let t = salida(&o);
    assert!(t.contains("API_KEY") && t.contains("npx") && t.contains("solo los nombres"), "{t}");
    sin_secretos(&t, SECRETOS);
    let v = m.json(&["biblioteca", "ver", SKILL, "--json"]);
    assert_eq!(v["id"], SKILL);
    assert_eq!(v["uso"]["total"], 1);
    assert_eq!(v["uso"]["por"][0]["actor"], "claude-code");
    let o = m.ea(&["biblioteca", "ver", "claude-code/skill/usuario/fantasma"]);
    assert!(!o.status.success());
}

#[test]
fn uso_en_json_por_elemento_y_general() {
    let m = mundo();
    let v = m.json(&["biblioteca", "uso", "--json"]);
    let u = v["usos"].as_array().unwrap().iter().find(|u| u["id"] == SKILL).expect("uso de la skill");
    assert_eq!(u["total"], 1);
    assert!(v["sin_elemento"].is_array() && v["actividad"].is_array());
    let v = m.json(&["biblioteca", "uso", SKILL, "--json"]);
    assert_eq!((v["id"].as_str(), v["total"].as_u64()), (Some(SKILL), Some(1)));
    let v = m.json(&["biblioteca", "uso", MCP, "--json"]);
    assert_eq!(v["total"], 0);
    let o = m.ea(&["biblioteca", "uso"]);
    assert!(o.status.success() && salida(&o).contains(SKILL), "{}", salida(&o));
}

#[test]
fn desactivar_sin_terminal_solo_propone() {
    let m = mundo();
    let o = m.ea(&["biblioteca", "desactivar", SKILL]);
    assert!(o.status.success(), "{}", salida(&o));
    let t = salida(&o);
    assert!(t.contains("Propuesta #1 creada") && t.contains("No se ha hecho nada"), "{t}");
    let p = m.propuestas();
    assert_eq!(p.len(), 1);
    let a = p[0].accion.as_ref().unwrap();
    assert_eq!(a.herramienta, "ea_desactivar_herramienta");
    assert_eq!(p[0].proponente.as_deref(), Some("usuario"));
    assert!(m.ruta("home/.claude/skills/propia/SKILL.md").exists(), "no se movió nada");
    // Repetir no duplica; con --esperar y sin terminal tampoco acepta.
    let o = m.ea(&["biblioteca", "desactivar", SKILL, "--esperar"]);
    assert!(o.status.success(), "{}", salida(&o));
    assert!(salida(&o).contains("ya estaba pendiente") && salida(&o).contains("Sin terminal interactiva"), "{}", salida(&o));
    assert_eq!(m.propuestas().len(), 1);
    assert!(m.ruta("home/.claude/skills/propia/SKILL.md").exists());
    // Activar algo que no existe falla sin dejar propuesta.
    let o = m.ea(&["biblioteca", "activar", "claude-code/skill/usuario/fantasma"]);
    assert!(!o.status.success());
    assert_eq!(m.propuestas().len(), 1);
}

#[test]
fn sesion_propone_sin_abrir_terminal_ni_escribir_ficheros() {
    let m = mundo();
    let o = m.ea(&["biblioteca", "sesion", "claude-code", MCP, SKILL, "--directorio", "."]);
    assert!(o.status.success(), "{}", salida(&o));
    let t = salida(&o);
    assert!(t.contains("Comando:") && t.contains("--strict-mcp-config") && t.contains("Propuesta #1 creada"), "{t}");
    sin_secretos(&t, SECRETOS);
    let p = m.propuestas();
    assert_eq!(p.len(), 1);
    let a = p[0].accion.as_ref().unwrap();
    assert_eq!(a.herramienta, "ea_lanzar_sesion");
    sin_secretos(&format!("{a:?}"), SECRETOS);
    // El directorio relativo se guardó absoluto.
    let dir = a.argumentos["directorio"].as_str().unwrap();
    assert!(Path::new(dir).is_absolute(), "{dir}");
    assert!(!m.ruta("xdg/rt/ea-biblioteca").exists(), "proponer no escribe la sesión");

    let o = m.ea(&["biblioteca", "sesion", "antigravity", MCP]);
    assert!(!o.status.success());
    assert_eq!(m.propuestas().len(), 1);
}

#[test]
fn categoria_sin_terminal_no_escribe_nada() {
    let m = mundo();
    let o = m.ea(&["biblioteca", "categoria", SKILL, "mis-cosas"]);
    assert!(!o.status.success());
    assert!(salida(&o).contains("terminal interactiva"), "{}", salida(&o));
    assert!(!m.ruta("config/ajustes.toml").exists());
    let o = m.ea(&["biblioteca", "categoria", SKILL]);
    assert!(!o.status.success(), "sin categoría ni --quitar es un error de uso");
}
