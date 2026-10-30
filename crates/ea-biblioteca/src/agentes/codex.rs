//! Codex (formato comprobado con 0.159.3).
//!
//! - skills: `~/.codex/skills/<n>/SKILL.md` y `<proyecto>/.agents/skills`
//!   (las ocultas, como `.system`, se saltan);
//! - MCP: `[mcp_servers.<n>]` de `~/.codex/config.toml`; `enabled` (activado
//!   por defecto) es el interruptor (mecanismo `campo`). Si el TOML no se
//!   interpreta, lista vacía con aviso.

use serde_json::Value;

use super::comun::{leer_texto, mcp as crear_mcp, skills_en};
use crate::modelo::{Activado, Ambito, Elemento, Mecanismo};
use crate::raices::Raices;

pub const AGENTE: &str = ea_ajustes::AgenteId::Codex.as_str();
pub const VERSION: &str = "0.159.3";

pub fn leer(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let repo = r.repo_efectivo();
    let mut sal = skills_en(&r.home.join(".codex/skills"), AGENTE, Ambito::Usuario, repo.as_deref(), Mecanismo::Mover, VERSION, avisos);
    if let Some(p) = &r.proyecto {
        sal.extend(skills_en(&p.join(".agents/skills"), AGENTE, Ambito::Proyecto, repo.as_deref(), Mecanismo::Mover, VERSION, avisos));
    }
    let config = r.home.join(".codex/config.toml");
    if let Some(texto) = leer_texto(&config) {
        match texto.parse::<toml::Table>() {
            Ok(t) => {
                if let Some(servidores) = t.get("mcp_servers").and_then(|v| v.as_table()) {
                    for (n, def) in servidores {
                        let Ok(json) = serde_json::to_value(def) else { continue };
                        if let Some(mut e) = crear_mcp(avisos, AGENTE, Ambito::Usuario, n, &config, &json, VERSION) {
                            e.activado = Activado::desde_bool(json.get("enabled").and_then(Value::as_bool).unwrap_or(true));
                            e.mecanismo = Mecanismo::Campo;
                            sal.push(e);
                        }
                    }
                }
            }
            Err(e) => avisos.push(format!("{} no es TOML válido ({}); se ignora", config.display(), e.message())),
        }
    }
    sal
}
