//! Qwen Code (formato comprobado con 0.25.1).
//!
//! - MCP: `mcpServers` de `~/.qwen/settings.json` (y `<proyecto>/.qwen/settings.json`).
//!   Activado salvo que figure en `mcp.excluded` o falte en `mcp.allowed`
//!   (si existe y no está vacío). Sin interruptor verificado: mecanismo `no`;
//! - extensiones: `~/.qwen/extensions/<n>/` (`qwen-extension.json` o
//!   `gemini-extension.json`) con `extension-enablement.json`
//!   (`{ "<n>": { "overrides": ["/ruta/*", "!/otra/*"] } }`; sin entrada, activada;
//!   gana el último override que cubre el proyecto o, si no, el HOME).
//!   Mecanismo `comando-oficial` (`qwen extensions enable|disable`);
//! - skills y subagentes de fichero (`~/.qwen/skills/<n>/SKILL.md`,
//!   `~/.qwen/agents/*.md`, y lo mismo en `<proyecto>/.qwen`): sin verificar
//!   contra 0.25.1, vacío si no existen;
//! - skills del EJECUTOR del ecosistema: los `*.md` de `agente.skills.dir`
//!   (ajustes), ámbito `ecosistema`, mecanismo `ajustes`; activadas si
//!   `agente.skills.habilitadas` está vacío o las nombra. Se leen por ruta con
//!   `ea-ajustes` porque `ea-bus` no es dependencia de este crate.

use std::path::Path;

use ea_ajustes::Ajustes;
use serde_json::Value;

use super::comun::{crear, frontmatter, leer_json, leer_texto, md_en, mcp as crear_mcp, recortar, skills_en};
use crate::modelo::{Activado, Ambito, Elemento, Mecanismo, Tipo};
use crate::raices::Raices;

pub const AGENTE: &str = "qwen-code";
pub const VERSION: &str = "0.25.1";

pub fn leer(r: &Raices, a: &Ajustes, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let repo = r.repo_efectivo();
    let mut sal = Vec::new();
    let mut bases = vec![(r.home.join(".qwen"), Ambito::Usuario)];
    if let Some(p) = &r.proyecto {
        bases.push((p.join(".qwen"), Ambito::Proyecto));
    }
    for (base, ambito) in &bases {
        let fichero = base.join("settings.json");
        if let Some(j) = leer_json(&fichero, false, avisos) {
            sal.extend(mcp(&j, &fichero, ambito.clone(), avisos));
        }
        sal.extend(skills_en(&base.join("skills"), AGENTE, ambito.clone(), repo.as_deref(), Mecanismo::Mover, VERSION, avisos));
        sal.extend(md_en(&base.join("agents"), AGENTE, Tipo::Subagente, ambito.clone(), Mecanismo::Mover, VERSION, avisos));
    }
    sal.extend(extensiones(r, avisos));
    sal.extend(skills_ejecutor(a, avisos));
    sal
}

fn lista(j: &Value, a: &str, b: &str) -> Option<Vec<String>> {
    let v = j.get(a)?.get(b)?.as_array()?;
    Some(v.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
}

fn mcp(j: &Value, fichero: &Path, ambito: Ambito, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let excluidos = lista(j, "mcp", "excluded").unwrap_or_default();
    let permitidos = lista(j, "mcp", "allowed").filter(|l| !l.is_empty());
    let Some(m) = j.get("mcpServers").and_then(Value::as_object) else { return Vec::new() };
    let mut sal = Vec::new();
    for (n, def) in m {
        if let Some(mut e) = crear_mcp(avisos, AGENTE, ambito.clone(), n, fichero, def, VERSION) {
            let permitido = permitidos.as_ref().is_none_or(|p| p.contains(n));
            e.activado = Activado::desde_bool(permitido && !excluidos.contains(n));
            e.mecanismo = Mecanismo::No;
            sal.push(e);
        }
    }
    sal
}

fn extensiones(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let dir = r.home.join(".qwen/extensions");
    let enablement = leer_json(&dir.join("extension-enablement.json"), false, avisos);
    let cwd = r.proyecto.clone().unwrap_or_else(|| r.home.clone());
    let mut nombres: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).collect();
    nombres.sort_by_key(|e| e.file_name());
    let mut sal = Vec::new();
    for entrada in nombres {
        let ruta = entrada.path();
        let Some(nombre) = entrada.file_name().to_str().map(str::to_string) else { continue };
        if nombre.starts_with('.') {
            continue;
        }
        let mut manifiesto = Value::Null;
        for f in ["qwen-extension.json", "gemini-extension.json"] {
            if let Some(j) = leer_json(&ruta.join(f), false, avisos) {
                manifiesto = j;
                break;
            }
        }
        let Some(mut e) = crear(avisos, AGENTE, Tipo::Extension, Ambito::Usuario, &nombre, ruta, VERSION) else { continue };
        e.descripcion = recortar(manifiesto.get("description").and_then(Value::as_str).unwrap_or(""));
        let overrides = enablement.as_ref().and_then(|j| j.get(&nombre)).and_then(|x| x.get("overrides")).and_then(Value::as_array);
        let mut activa = true;
        for o in overrides.into_iter().flatten().filter_map(Value::as_str) {
            let (desactiva, patron) = o.strip_prefix('!').map_or((false, o), |p| (true, p));
            let base = patron.trim_end_matches('*').trim_end_matches('/');
            if cwd.starts_with(if base.is_empty() { "/" } else { base }) {
                activa = !desactiva;
            }
        }
        e.activado = Activado::desde_bool(activa);
        e.mecanismo = Mecanismo::ComandoOficial;
        sal.push(e);
    }
    sal
}

fn skills_ejecutor(a: &Ajustes, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let dir = Path::new(&a.agente.skills.dir);
    let mut ficheros: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    ficheros.sort();
    let mut sal = Vec::new();
    for ruta in ficheros {
        let Some(nombre) = ruta.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".md")).map(str::to_string) else { continue };
        if nombre.starts_with('.') || !ruta.is_file() {
            continue;
        }
        let Some(mut e) = crear(avisos, AGENTE, Tipo::Skill, Ambito::Ecosistema, &nombre, ruta.clone(), VERSION) else { continue };
        let fm = leer_texto(&ruta).map(|t| frontmatter(&t)).unwrap_or_default();
        e.descripcion = recortar(fm.get("descripcion").or_else(|| fm.get("description")).map_or("", String::as_str));
        let hab = &a.agente.skills.habilitadas;
        e.activado = Activado::desde_bool(hab.is_empty() || hab.contains(&nombre));
        e.mecanismo = Mecanismo::Ajustes;
        sal.push(e);
    }
    sal
}
