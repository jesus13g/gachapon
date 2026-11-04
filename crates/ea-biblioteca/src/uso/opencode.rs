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
    "read",
    "write",
    "edit",
    "multiedit",
    "bash",
    "glob",
    "grep",
    "list",
    "ls",
    "patch",
    "apply_patch",
    "todowrite",
    "todoread",
    "webfetch",
    "websearch",
    "codesearch",
    "lsp",
    "question",
    "invalid",
    "batch",
    "plan_enter",
    "plan_exit",
    "execute",
    "shell",
];

/// Lo que se lleva visto de la base: la marca (`time_created` más alto) y los
/// ids de las filas que tienen justo esa marca. Varias filas pueden compartir
/// el mismo milisegundo, así que la lectura incremental repite la marca (`>=`)
/// y descarta por id las que ya se contaron.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visto {
    pub marca: i64,
    pub ids: Vec<String>,
}

/// Hallazgos de las filas nuevas y lo visto hasta ahora.
pub fn leer(ruta: &Path, desde: &Visto, avisos: &mut Vec<String>) -> (Vec<Hallazgo>, Visto) {
    if !ruta.is_file() {
        return (Vec::new(), desde.clone());
    }
    let r = (|| -> rusqlite::Result<(Vec<Hallazgo>, Visto)> {
        let c = Connection::open_with_flags(ruta, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        c.busy_timeout(std::time::Duration::from_millis(500))?;
        let mut st =
            c.prepare("SELECT id, data, time_created FROM session_message WHERE type = 'assistant' AND time_created >= ?1 ORDER BY time_created, id")?;
        let mut filas = st.query([desde.marca])?;
        let mut sal = Vec::new();
        let mut visto = desde.clone();
        while let Some(f) = filas.next()? {
            let (id, data, t): (String, String, i64) = (f.get(0)?, f.get(1)?, f.get(2)?);
            // Ya contada en la pasada anterior (misma marca).
            if t == desde.marca && desde.ids.contains(&id) {
                continue;
            }
            if t > visto.marca {
                visto = Visto { marca: t, ids: Vec::new() };
            }
            if t == visto.marca {
                visto.ids.push(id);
            }
            sal.extend(extraer(&data, t));
        }
        Ok((sal, visto))
    })();
    r.unwrap_or_else(|e| {
        avisos.push(format!("{}: no se pudo leer OpenCode ({e})", ruta.display()));
        (Vec::new(), desde.clone())
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

#[cfg(test)]
mod pruebas {
    use super::*;

    fn base(ruta: &Path) -> Connection {
        let c = Connection::open(ruta).unwrap();
        c.execute_batch("CREATE TABLE session_message (id text PRIMARY KEY, type text NOT NULL, time_created integer NOT NULL, data text NOT NULL);").unwrap();
        c
    }

    fn fila(c: &Connection, id: &str, t: i64, herramienta: &str) {
        let data = format!(r#"{{"content":[{{"type":"tool","name":"{herramienta}"}}]}}"#);
        c.execute("INSERT INTO session_message (id, type, time_created, data) VALUES (?1, 'assistant', ?2, ?3)", rusqlite::params![id, t, data]).unwrap();
    }

    #[test]
    fn filas_del_mismo_milisegundo_no_se_pierden_ni_se_cuentan_dos_veces() {
        let d = tempfile::tempdir().unwrap();
        let ruta = d.path().join("opencode.db");
        let c = base(&ruta);
        let mut avisos = Vec::new();
        fila(&c, "a", 1000, "srv_uno");
        let (h1, v1) = leer(&ruta, &Visto::default(), &mut avisos);
        assert_eq!(h1.len(), 1);
        assert_eq!(v1, Visto { marca: 1000, ids: vec!["a".into()] });

        // Otra fila en el MISMO milisegundo, llegada después de la primera lectura.
        fila(&c, "b", 1000, "srv_dos");
        let (h2, v2) = leer(&ruta, &v1, &mut avisos);
        assert_eq!(h2.iter().map(|h| h.nombre.as_str()).collect::<Vec<_>>(), ["srv_dos"], "no se pierde la fila b ni se repite la a");
        assert_eq!(v2.marca, 1000);
        assert_eq!(v2.ids.len(), 2);

        // Sin nada nuevo no se cuenta nada; una fila posterior avanza la marca.
        assert!(leer(&ruta, &v2, &mut avisos).0.is_empty());
        fila(&c, "c", 1001, "srv_tres");
        let (h3, v3) = leer(&ruta, &v2, &mut avisos);
        assert_eq!(h3.len(), 1);
        assert_eq!(v3, Visto { marca: 1001, ids: vec!["c".into()] });
        assert!(avisos.is_empty(), "{avisos:?}");
    }
}
