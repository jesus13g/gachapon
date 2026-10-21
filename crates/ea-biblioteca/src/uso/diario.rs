//! Diario del ecosistema (SQLite, tabla `events`): los `mcp_*` son llamadas
//! al servidor `ea-mcp`. Se abre en solo lectura con `rusqlite` directo y no
//! con `ea-store`: este abre con migraciones y rechaza esquemas ajenos, y la
//! biblioteca no debe poder escribir ni crear nada ahí.
//!
//! El actor es `detail.agente` si existe y, si no, la columna `actor`.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::parse_ts;

/// `(actor, usos, último en ms)` de las acciones `mcp_*`.
pub fn leer(ruta: &Path, avisos: &mut Vec<String>) -> Vec<(String, u64, Option<i64>)> {
    if !ruta.is_file() {
        return Vec::new();
    }
    let r = (|| -> rusqlite::Result<Vec<(String, u64, Option<String>)>> {
        let c = Connection::open_with_flags(ruta, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        c.busy_timeout(std::time::Duration::from_millis(500))?;
        let mut st = c.prepare(
            "SELECT coalesce(CASE WHEN json_valid(detail) THEN json_extract(detail, '$.agente') END, actor) AS a,
                    count(*), max(ts)
             FROM events WHERE action LIKE 'mcp\\_%' ESCAPE '\\' GROUP BY a",
        )?;
        st.query_map([], |f| Ok((f.get::<_, String>(0)?, f.get::<_, i64>(1)? as u64, f.get::<_, Option<String>>(2)?)))?.collect()
    })();
    match r {
        Ok(v) => v.into_iter().map(|(a, n, ts)| (a, n, ts.as_deref().and_then(parse_ts))).collect(),
        Err(e) => {
            avisos.push(format!("{}: no se pudo leer el diario ({e})", ruta.display()));
            Vec::new()
        }
    }
}
