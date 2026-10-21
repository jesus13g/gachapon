//! Claude Code: `~/.claude.json` (contadores propios) y transcripciones
//! `~/.claude/projects/**/*.jsonl` (incluidas las de `<sesión>/subagents/`).
//!
//! De una transcripción solo se extrae, por cada bloque `tool_use`, el nombre
//! de la skill, del subagente o del servidor MCP, más el `timestamp` de la
//! línea. El resto de la línea se descarta sin guardarlo.
//!
//! No se usa el `agentType` de los `subagents/*.meta.json`: el `tool_use` de
//! `Agent` en la transcripción padre ya cuenta ese mismo uso.

use serde_json::Value;

use super::{Hallazgo, parse_ts};
use crate::modelo::Tipo;

/// Lo que cuentan los `tool_use` de una línea de transcripción de Claude Code.
pub fn extraer(linea: &str) -> Vec<Hallazgo> {
    if !linea.contains("\"tool_use\"") {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(linea) else { return Vec::new() };
    let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_ts);
    let Some(bloques) = v.pointer("/message/content").and_then(Value::as_array) else { return Vec::new() };
    let mut sal = Vec::new();
    for b in bloques {
        if b.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let Some(nombre) = b.get("name").and_then(Value::as_str) else { continue };
        let entrada = b.get("input");
        let campo = |c: &str| entrada.and_then(|i| i.get(c)).and_then(Value::as_str).filter(|s| !s.is_empty());
        let (tipo, nombre) = match nombre {
            "Skill" => match campo("skill") {
                Some(s) => (Tipo::Skill, s.trim_start_matches('/').to_string()),
                None => continue,
            },
            "Agent" | "Task" => (Tipo::Subagente, campo("subagent_type").unwrap_or("general-purpose").to_string()),
            n => match servidor_mcp(n) {
                Some(s) => (Tipo::Mcp, s.to_string()),
                None => continue,
            },
        };
        sal.push(Hallazgo { tipo, nombre, ts_ms: ts });
    }
    sal
}

/// `mcp__<servidor>__<herramienta>` → `<servidor>`.
pub fn servidor_mcp(nombre: &str) -> Option<&str> {
    let resto = nombre.strip_prefix("mcp__")?;
    let s = resto.split_once("__").map(|(s, _)| s).unwrap_or(resto);
    (!s.is_empty()).then_some(s)
}

/// Contadores de `~/.claude.json`: `(nombre, usos, último en ms)` por skill,
/// plugin y subagente. `None` si no existe o no se pudo leer (en ese caso queda
/// un aviso); el fichero puede estar a medio escribir, así que se reintenta
/// antes de rendirse. Solo se miran tres claves; el resto (que lleva secretos)
/// no se toca.
pub fn contadores(ruta: &std::path::Path, avisos: &mut Vec<String>) -> Option<Contadores> {
    if !ruta.exists() {
        return None;
    }
    let mut v: Option<Value> = None;
    for intento in 0..4 {
        if let Ok(b) = std::fs::read(ruta)
            && let Ok(j) = serde_json::from_slice::<Value>(&b)
        {
            v = Some(j);
            break;
        }
        if intento < 3 {
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
    }
    let Some(v) = v else {
        avisos.push(format!("{}: no se pudo interpretar (¿a medio escribir?); se omiten sus contadores de uso", ruta.display()));
        return None;
    };
    let uso = |clave: &str| -> Vec<(String, u64, Option<i64>)> {
        v.get(clave)
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(n, d)| {
                (
                    n.trim_start_matches('/').to_string(),
                    d.get("usageCount").and_then(Value::as_u64).unwrap_or(0),
                    d.get("lastUsedAt").and_then(Value::as_i64),
                )
            })
            .collect()
    };
    let subagentes = v
        .get("agentLastUsed")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(n, d)| (n.clone(), 0, d.as_i64()))
        .collect();
    Some(Contadores { skills: uso("skillUsage"), plugins: uso("pluginUsage"), subagentes })
}

#[derive(Debug, Default)]
pub struct Contadores {
    pub skills: Vec<(String, u64, Option<i64>)>,
    pub plugins: Vec<(String, u64, Option<i64>)>,
    pub subagentes: Vec<(String, u64, Option<i64>)>,
}
