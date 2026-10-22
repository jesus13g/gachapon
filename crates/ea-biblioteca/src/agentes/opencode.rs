//! OpenCode (formato comprobado con 2.1.1).
//!
//! - configuración: `~/.config/opencode/opencode.json` y `.jsonc`, y
//!   `<proyecto>/opencode.json(c)`. El JSONC se lee quitando comentarios; si
//!   llevaba comentarios, sus elementos quedan con mecanismo `no` y una nota
//!   (la F4 no reescribe un fichero con comentarios);
//! - `mcp.<n>` y la forma `mcp.servers.<n>` (se toma `servers` como
//!   contenedor si es un objeto que no parece un servidor); `enabled`
//!   (activado por defecto) es el interruptor (mecanismo `campo`);
//! - `plugin` (array de nombres o `[nombre, opciones]`): activos mientras
//!   figuren; `agent` (objeto, `disable: true` los apaga);
//! - ficheros: `{agent,agents}/*.md` (subagentes), `{command,commands}/*.md`
//!   (comandos) y `{skill,skills}/<n>/SKILL.md`, en `~/.config/opencode` y en
//!   `<proyecto>/.opencode`, con mecanismo `mover`.
//!
//! El servicio de OpenCode en segundo plano no relee su configuración.

use std::path::Path;

use serde_json::Value;

use super::comun::{crear, leer_jsonc, md_en, mcp as crear_mcp, recortar, skills_en};
use crate::modelo::{Activado, Ambito, Elemento, Mecanismo, Tipo};
use crate::raices::Raices;
use crate::redactar::{redactar_texto, redactar_url};

pub const AGENTE: &str = "opencode";
pub const VERSION: &str = "2.1.1";

pub fn leer(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let repo = r.repo_efectivo();
    let mut sal = Vec::new();
    // OpenCode usa `~/.config/opencode` (XDG) también en macOS y Windows.
    let mut sitios = vec![(r.home.join(".config/opencode"), r.home.join(".config/opencode"), Ambito::Usuario)];
    if let Some(p) = &r.proyecto {
        sitios.push((p.clone(), p.join(".opencode"), Ambito::Proyecto));
    }
    for (dir_config, dir_ficheros, ambito) in sitios {
        for nombre in ["opencode.json", "opencode.jsonc"] {
            let fichero = dir_config.join(nombre);
            if let Some((j, con_comentarios)) = leer_jsonc(&fichero, avisos) {
                sal.extend(config(&j, &fichero, con_comentarios, ambito.clone(), avisos));
            }
        }
        for d in ["agent", "agents"] {
            sal.extend(md_en(&dir_ficheros.join(d), AGENTE, Tipo::Subagente, ambito.clone(), Mecanismo::Mover, VERSION, avisos));
        }
        for d in ["command", "commands"] {
            sal.extend(md_en(&dir_ficheros.join(d), AGENTE, Tipo::Comando, ambito.clone(), Mecanismo::Mover, VERSION, avisos));
        }
        for d in ["skill", "skills"] {
            sal.extend(skills_en(&dir_ficheros.join(d), AGENTE, ambito.clone(), repo.as_deref(), Mecanismo::Mover, VERSION, avisos));
        }
    }
    sal
}

fn parece_servidor(v: &Value) -> bool {
    ["type", "command", "url", "enabled"].iter().any(|k| v.get(*k).is_some())
}

fn config(j: &Value, fichero: &Path, con_comentarios: bool, ambito: Ambito, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let nota = con_comentarios.then(|| "JSONC con comentarios: no se edita (se rechaza el cambio)".to_string());
    let mut sal = Vec::new();
    let mecanismo = if con_comentarios { Mecanismo::No } else { Mecanismo::Campo };

    if let Some(mcp) = j.get("mcp").and_then(Value::as_object) {
        let contenedor = match mcp.get("servers") {
            Some(s) if s.is_object() && !parece_servidor(s) => s.as_object().expect("es objeto"),
            _ => mcp,
        };
        for (n, def) in contenedor {
            if !def.is_object() {
                continue;
            }
            if let Some(mut e) = crear_mcp(avisos, AGENTE, ambito.clone(), n, fichero, def, VERSION) {
                e.activado = Activado::desde_bool(def.get("enabled").and_then(Value::as_bool).unwrap_or(true));
                e.mecanismo = mecanismo;
                e.nota = nota.clone();
                sal.push(e);
            }
        }
    }
    if let Some(plugins) = j.get("plugin").or_else(|| j.get("plugins")).and_then(Value::as_array) {
        for p in plugins {
            let Some(nombre) = p.as_str().or_else(|| p.as_array().and_then(|a| a.first()).and_then(Value::as_str)) else { continue };
            // Un plugin puede ser una URL o una ruta: sin credenciales ni consulta.
            let nombre = if nombre.contains("://") { redactar_url(nombre) } else { redactar_texto(nombre) };
            if let Some(mut e) = crear(avisos, AGENTE, Tipo::Plugin, ambito.clone(), &nombre, fichero.to_path_buf(), VERSION) {
                e.activado = Activado::Si;
                e.nota = nota.clone();
                sal.push(e);
            }
        }
    }
    if let Some(agentes) = j.get("agent").and_then(Value::as_object) {
        for (n, def) in agentes {
            if let Some(mut e) = crear(avisos, AGENTE, Tipo::Subagente, ambito.clone(), n, fichero.to_path_buf(), VERSION) {
                e.descripcion = recortar(def.get("description").and_then(Value::as_str).unwrap_or(""));
                e.activado = Activado::desde_bool(def.get("disable").and_then(Value::as_bool) != Some(true));
                e.nota = nota.clone();
                sal.push(e);
            }
        }
    }
    sal
}
