//! Antigravity (`agy`, formato comprobado con 1.4.4).
//!
//! - skills: `~/.gemini/config/skills/<n>/` (mover) y las integradas
//!   `~/.gemini/antigravity-cli/builtin/skills` (ámbito `integrada`, sin cambio);
//! - MCP: `mcpServers.<n>` de `~/.gemini/config/mcp_config.json`; `disabled`
//!   apaga (mecanismo `comando-oficial`: `agy mcp enable|disable`).
//! - Plugins y subagentes (`agy plugin list --json`, `agy agents`) requieren
//!   ejecutar el binario y no se inventarían aquí.

use serde_json::Value;

use super::comun::{leer_json, mcp as crear_mcp, skills_en};
use crate::modelo::{Activado, Ambito, Elemento, Mecanismo};
use crate::raices::Raices;

pub const AGENTE: &str = "antigravity";
pub const VERSION: &str = "1.4.4";

pub fn leer(r: &Raices, avisos: &mut Vec<String>) -> Vec<Elemento> {
    let repo = r.repo_efectivo();
    let mut sal = skills_en(&r.home.join(".gemini/config/skills"), AGENTE, Ambito::Usuario, repo.as_deref(), Mecanismo::Mover, VERSION, avisos);
    sal.extend(skills_en(&r.home.join(".gemini/antigravity-cli/builtin/skills"), AGENTE, Ambito::Integrada, None, Mecanismo::No, VERSION, avisos));
    let config = r.home.join(".gemini/config/mcp_config.json");
    if let Some(j) = leer_json(&config, false, avisos)
        && let Some(m) = j.get("mcpServers").and_then(Value::as_object)
    {
        for (n, def) in m {
            if let Some(mut e) = crear_mcp(avisos, AGENTE, Ambito::Usuario, n, &config, def, VERSION) {
                e.activado = Activado::desde_bool(def.get("disabled").and_then(Value::as_bool) != Some(true));
                e.mecanismo = Mecanismo::ComandoOficial;
                sal.push(e);
            }
        }
    }
    sal
}
