//! Qwen Code: `~/.qwen/projects/*/chats/*.jsonl`. Cada línea de tipo
//! `assistant` lleva `message.parts[].functionCall{name,args}` y `timestamp`.
//! Los servidores MCP llegan como `mcp__<servidor>__<herramienta>`; la skill
//! y el subagente, como las herramientas `skill` y `task` (no verificado con
//! datos reales: en las transcripciones locales solo hay herramientas de
//! fichero y shell).

use serde_json::Value;

use super::{Hallazgo, claude::servidor_mcp, parse_ts};
use crate::modelo::Tipo;

pub fn extraer(linea: &str) -> Vec<Hallazgo> {
    if !linea.contains("functionCall") {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(linea) else { return Vec::new() };
    let ts = v.get("timestamp").and_then(Value::as_str).and_then(parse_ts);
    let Some(partes) = v.pointer("/message/parts").and_then(Value::as_array) else { return Vec::new() };
    let mut sal = Vec::new();
    for p in partes {
        let Some(f) = p.get("functionCall") else { continue };
        let Some(nombre) = f.get("name").and_then(Value::as_str) else { continue };
        let arg = |c: &str| f.pointer(&format!("/args/{c}")).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let (tipo, nombre) = match nombre {
            "skill" => match arg("skill").or_else(|| arg("name")) {
                Some(s) => (Tipo::Skill, s),
                None => continue,
            },
            "task" => match arg("subagent_type") {
                Some(s) => (Tipo::Subagente, s),
                None => continue,
            },
            n => match servidor_mcp(n) {
                Some(s) => (Tipo::Mcp, s.to_string()),
                None => continue,
            },
        };
        sal.push(Hallazgo { tipo, nombre, ts_ms: ts });
    }
    sal
}
