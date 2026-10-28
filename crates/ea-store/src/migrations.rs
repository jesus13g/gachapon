use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::error::{StoreError, StoreResult};

/// Versión del esquema que conoce este binario (número de migraciones).
const VERSION_CONOCIDA: i64 = 1;

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("../migrations/0001_inicial.sql"))])
}

/// Rechaza una base del esquema antiguo (con la tabla `tasks` o con una
/// versión mayor de la conocida). Nunca la toca: la decisión de copiarla y
/// borrarla es del usuario.
pub(crate) fn comprobar_esquema(conn: &Connection) -> StoreResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let con_tasks: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tasks')",
        [],
        |r| r.get(0),
    )?;
    if version > VERSION_CONOCIDA || con_tasks {
        return Err(StoreError::EsquemaAntiguo);
    }
    Ok(())
}

pub fn run_migrations(conn: &mut Connection) -> StoreResult<()> {
    comprobar_esquema(conn)?;
    migrations().to_latest(conn)?;
    Ok(())
}
