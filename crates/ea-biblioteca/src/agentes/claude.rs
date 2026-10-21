//! Claude Code (formato comprobado con 2.1.301).
//!
//! - skills: `~/.claude/skills/<n>/SKILL.md` y `<proyecto>/.claude/skills`;
//!   un enlace al `claude/skills` del repo cuenta como `ecosistema`;
//! - subagentes y comandos: `agents/*.md` y `commands/*.md` de `~/.claude` y
//!   de `<proyecto>/.claude`;
//! - plugins: `~/.claude/plugins/installed_plugins.json` (v2) con el estado de
//!   `enabledPlugins` (settings de usuario, proyecto y local). Instalado y sin
//!   entrada en `enabledPlugins` es `no`. De los plugins activos se listan sus
//!   skills, subagentes y comandos (ámbito `plugin:<id>`, sin interruptor
//!   propio: su estado es el del plugin);
//! - MCP: `~/.claude.json` (`mcpServers` y `projects[<proyecto>].mcpServers`)
//!   y `<proyecto>/.mcp.json`, siempre `solo-sesion`. `~/.claude.json` solo se
//!   lee y puede estar a medio escribir: un fallo de lectura es una lista
//!   vacía con aviso. Los MCP que traen los plugins no se listan.

use std::path::Path;

use serde_json::Value;

use super::comun::{crear, mcp as crear_mcp, leer_json, md_en, skills_en};
use crate::modelo::{Activado, Ambito, Elemento, Mecanismo, Tipo};
use crate::raices::Raices;

pub const AGENTE: &str = "claude-code";
pub const VERSION: &str = "2.1.301";

pub fn leer(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let repo = r.repo_efectivo();
    let repo = repo.as_deref();
    let mut sal = Vec::new();
    let mut bases = vec![(r.home.join(".claude"), Ambito::Usuario)];
    if let Some(p) = &r.proyecto {
        bases.push((p.join(".claude"), Ambito::Proyecto));
    }
    for (base, ambito) in &bases {
        sal.extend(skills_en(&base.join("skills"), AGENTE, ambito.clone(), repo, Mecanismo::Mover, VERSION, avisos));
        sal.extend(md_en(&base.join("agents"), AGENTE, Tipo::Subagente, ambito.clone(), Mecanismo::Mover, VERSION, avisos));
        sal.extend(md_en(&base.join("commands"), AGENTE, Tipo::Comando, ambito.clone(), Mecanismo::Mover, VERSION, avisos));
    }
    sal.extend(plugins(r, avisos));
    sal.extend(mcp(r, avisos));
    sal
}

/// `enabledPlugins` fusionado: usuario, luego proyecto, luego local.
fn habilitados(r: &Raices, avisos: &mut Vec<String>) -> (std::collections::BTreeMap<String, bool>, bool) {
    let mut ficheros = vec![r.home.join(".claude/settings.json")];
    if let Some(p) = &r.proyecto {
        ficheros.push(p.join(".claude/settings.json"));
        ficheros.push(p.join(".claude/settings.local.json"));
    }
    let mut mapa = std::collections::BTreeMap::new();
    let mut alguno = false;
    for f in ficheros {
        let Some(j) = leer_json(&f, false, avisos) else { continue };
        alguno = true;
        if let Some(m) = j.get("enabledPlugins").and_then(Value::as_object) {
            for (k, v) in m {
                if let Some(b) = v.as_bool() {
                    mapa.insert(k.clone(), b);
                }
            }
        }
    }
    (mapa, alguno)
}

fn plugins(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let ruta = r.home.join(".claude/plugins/installed_plugins.json");
    let Some(j) = leer_json(&ruta, false, avisos) else { return Vec::new() };
    let Some(instalados) = j.get("plugins").and_then(Value::as_object) else { return Vec::new() };
    let (estado, hay_settings) = habilitados(r, avisos);
    let mut sal = Vec::new();
    for (id, entradas) in instalados {
        let Some(lista) = entradas.as_array() else { continue };
        // Una instalación de usuario, o una de proyecto que sea de este proyecto.
        let aplicable = |e: &Value| match e.get("scope").and_then(Value::as_str).unwrap_or("user") {
            "user" => Some(Ambito::Usuario),
            _ => {
                let pp = e.get("projectPath").and_then(Value::as_str)?;
                (r.proyecto.as_deref() == Some(Path::new(pp))).then_some(Ambito::Proyecto)
            }
        };
        let Some((entrada, ambito)) = lista.iter().find_map(|e| aplicable(e).map(|a| (e, a))) else { continue };
        let Some(mut e) = crear(avisos, AGENTE, Tipo::Plugin, ambito, id, ruta.clone(), VERSION) else { continue };
        let activado = match estado.get(id) {
            Some(b) => Activado::desde_bool(*b),
            None if hay_settings => Activado::No,
            None => Activado::Desconocido,
        };
        e.activado = activado;
        e.mecanismo = Mecanismo::ComandoOficial;
        let version = entrada.get("version").and_then(Value::as_str).unwrap_or("");
        let instalacion = entrada.get("installPath").and_then(Value::as_str).map(std::path::PathBuf::from);
        e.descripcion = instalacion.as_deref().map(|d| descripcion_plugin(d)).filter(|d| !d.is_empty()).unwrap_or_else(|| version.to_string());
        sal.push(e);
        // Lo que trae el plugin: solo si está en disco (barato) y no se ha pedido su exclusión.
        if let Some(dir) = instalacion.filter(|d| d.is_dir()) {
            let ambito = Ambito::Plugin(id.clone());
            let mut dentro = skills_en(&dir.join("skills"), AGENTE, ambito.clone(), None, Mecanismo::No, VERSION, avisos);
            dentro.extend(md_en(&dir.join("agents"), AGENTE, Tipo::Subagente, ambito.clone(), Mecanismo::No, VERSION, avisos));
            dentro.extend(md_en(&dir.join("commands"), AGENTE, Tipo::Comando, ambito, Mecanismo::No, VERSION, avisos));
            for d in &mut dentro {
                d.activado = activado;
            }
            sal.extend(dentro);
        }
    }
    sal
}

fn descripcion_plugin(dir: &Path) -> String {
    let mut sink = Vec::new();
    leer_json(&dir.join(".claude-plugin/plugin.json"), false, &mut sink)
        .and_then(|j| j.get("description").and_then(Value::as_str).map(super::comun::recortar))
        .unwrap_or_default()
}

fn nombres(v: &Value, clave: &str) -> Vec<String> {
    v.get(clave).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn mcp(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let mut sal = Vec::new();
    let config = r.home.join(".claude.json");
    let global = leer_json(&config, true, avisos);
    let clave_proyecto = r.proyecto.as_ref().map(|p| p.to_string_lossy().into_owned());
    let del_proyecto = global.as_ref().zip(clave_proyecto.as_ref()).and_then(|(g, p)| g.get("projects")?.get(p));
    let mut sin_permiso: Vec<String> = del_proyecto.map(|p| nombres(p, "disabledMcpServers")).unwrap_or_default();

    if let Some(g) = &global {
        if let Some(m) = g.get("mcpServers").and_then(Value::as_object) {
            for (n, def) in m {
                if let Some(mut e) = crear_mcp(avisos, AGENTE, Ambito::Usuario, n, &config, def, VERSION) {
                    e.activado = Activado::desde_bool(!sin_permiso.contains(n));
                    e.mecanismo = Mecanismo::SoloSesion;
                    sal.push(e);
                }
            }
        }
        if let Some(m) = del_proyecto.and_then(|p| p.get("mcpServers")).and_then(Value::as_object) {
            for (n, def) in m {
                if let Some(mut e) = crear_mcp(avisos, AGENTE, Ambito::Proyecto, n, &config, def, VERSION) {
                    e.activado = Activado::desde_bool(!sin_permiso.contains(n));
                    e.mecanismo = Mecanismo::SoloSesion;
                    sal.push(e);
                }
            }
        }
    }

    // `.mcp.json` del proyecto: hay que aprobarlo (enabled/disabled en los ajustes).
    if let Some(p) = &r.proyecto {
        let fichero = p.join(".mcp.json");
        if let Some(j) = leer_json(&fichero, false, avisos)
            && let Some(m) = j.get("mcpServers").and_then(Value::as_object)
        {
            let mut off = nombres_ajustes(r, del_proyecto, "disabledMcpjsonServers", avisos);
            let on = nombres_ajustes(r, del_proyecto, "enabledMcpjsonServers", avisos);
            off.append(&mut sin_permiso);
            let todos = todos_aprobados(r, avisos);
            for (n, def) in m {
                if let Some(mut e) = crear_mcp(avisos, AGENTE, Ambito::Proyecto, n, &fichero, def, VERSION) {
                    e.activado = if off.contains(n) {
                        Activado::No
                    } else if on.contains(n) || todos {
                        Activado::Si
                    } else {
                        Activado::Desconocido
                    };
                    e.mecanismo = Mecanismo::SoloSesion;
                    sal.push(e);
                }
            }
        }
    }
    sal
}

/// Listas de aprobación de `.mcp.json` en `~/.claude.json` del proyecto y en los settings.
fn nombres_ajustes(r: &Raices, del_proyecto: Option<&Value>, clave: &str, avisos: &mut Vec<String>) -> Vec<String> {
    let mut v = del_proyecto.map(|p| nombres(p, clave)).unwrap_or_default();
    for f in settings(r) {
        if let Some(j) = leer_json(&f, false, avisos) {
            v.extend(nombres(&j, clave));
        }
    }
    v
}

fn todos_aprobados(r: &Raices, avisos: &mut Vec<String>) -> bool {
    settings(r).iter().any(|f| leer_json(f, false, avisos).and_then(|j| j.get("enableAllProjectMcpServers").and_then(Value::as_bool)) == Some(true))
}

fn settings(r: &Raices) -> Vec<std::path::PathBuf> {
    let mut v = vec![r.home.join(".claude/settings.json")];
    if let Some(p) = &r.proyecto {
        v.push(p.join(".claude/settings.json"));
        v.push(p.join(".claude/settings.local.json"));
    }
    v
}
