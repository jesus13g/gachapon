//! OpenCode: `~/.local/share/opencode/opencode.db`, tabla `session_message`
//! (`type = 'assistant'`, `data.content[]` con `type = 'tool'` y `name`).
//! Es incremental por `time_created` (ms): la caché guarda el último visto.
//!
//! Las herramientas propias (`read`, `bash`…) no cuentan; `skill` y `task` se
//! leen de `state.input` (no verificado con datos reales: la base local no
//! tiene ninguna) y el resto se toma como `<servidor>_<herramienta>` de un
//! servidor MCP, que el resolutor casa con el inventario.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use super::Hallazgo;
use crate::modelo::Tipo;

const PROPIAS: &[&str] = &[
    "read", "write", "edit", "multiedit", "bash", "glob", "grep", "list", "ls", "patch", "apply_patch", "todowrite", "todoread", "webfetch",
    "websearch", "codesearch", "lsp", "question", "invalid", "batch", "plan_enter", "plan_exit", "execute", "shell",
];

/// Hallazgos de las filas nuevas y la marca más alta vista.
pub fn leer(ruta: &Path, desde: i64, avisos: &mut Vec<String>) -> (Vec<Hallazgo>, i64) {
    if !ruta.is_file() {
        return (Vec::new(), desde);
    }
    let r = (|| -> rusqlite::Result<(Vec<Hallazgo>, i64)> {
        let c = Connection::open_with_flags(ruta, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        c.busy_timeout(std::time::Duration::from_millis(500))?;
        let mut st = c.prepare("SELECT data, time_created FROM session_message WHERE type = 'assistant' AND time_created > ?1 ORDER BY time_created")?;
        let mut filas = st.query([desde])?;
        let (mut sal, mut marca) = (Vec::new(), desde);
        while let Some(f) = filas.next()? {
            let (data, t): (String, i64) = (f.get(0)?, f.get(1)?);
            marca = marca.max(t);
            sal.extend(extraer(&data, t));
        }
        Ok((sal, marca))
    })();
    r.unwrap_or_else(|e| {
        avisos.push(format!("{}: no se pudo leer OpenCode ({e})", ruta.display()));
        (Vec::new(), desde)
    })
}

fn extraer(data: &str, t: i64) -> Vec<Hallazgo> {
    let Ok(v) = serde_json::from_str::<Value>(data) else { return Vec::new() };
    let mut sal = Vec::new();
    for b in v.get("content").and_then(Value::as_array).into_iter().flatten() {
        if b.get("type").and_then(Value::as_str) != Some("tool") {
            continue;
        }
        let Some(nombre) = b.get("name").and_then(Value::as_str) else { continue };
        let entrada = |c: &str| b.pointer(&format!("/state/input/{c}")).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let (tipo, nombre) = match nombre {
            "skill" => match entrada("name") {
                Some(s) => (Tipo::Skill, s),
                None => continue,
            },
            "task" => match entrada("subagent_type") {
                Some(s) => (Tipo::Subagente, s),
                None => continue,
            },
            n if PROPIAS.contains(&n) => continue,
            n => (Tipo::Mcp, n.to_string()),
        };
        sal.push(Hallazgo { tipo, nombre, ts_ms: Some(t) });
    }
    sal
}

/// Última actividad: la sesión más recientemente actualizada.
pub fn actividad(ruta: &Path) -> Option<i64> {
    let c = Connection::open_with_flags(ruta, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    c.query_row("SELECT max(time_updated) FROM session_v2", [], |f| f.get::<_, Option<i64>>(0)).ok().flatten()
}
