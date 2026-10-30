use rusqlite::{Connection, OpenFlags};

use crate::error::{StoreError, StoreResult};
use crate::migrations::{comprobar_esquema, run_migrations};

/// Abre la base de datos en `path` (`rutas.db` de los ajustes, que resuelve
/// `$EA_DB_PATH`; `:memory:` en pruebas), crea su directorio si falta, aplica
/// los PRAGMAs necesarios y ejecuta las migraciones pendientes. Punto de
/// entrada único para todo consumidor de `ea-store`: la ruta la decide quien
/// llama, nunca esta capa.
pub fn open_at(path: &std::path::Path) -> StoreResult<Connection> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => std::fs::create_dir_all(dir)?,
        _ => {}
    }
    const INTENTOS: u32 = 5;
    let mut intento = 1;
    loop {
        match abrir_una_vez(path) {
            Ok(conn) => return Ok(conn),
            Err(StoreError::EsquemaAntiguo) => return Err(StoreError::EsquemaAntiguo),
            Err(_) if intento < INTENTOS => {
                std::thread::sleep(std::time::Duration::from_millis(100 * u64::from(intento)));
                intento += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Un intento de apertura. Puede fallar de forma pasajera cuando varios
/// procesos abren a la vez una base recién creada (los servicios al iniciar
/// sesión): el cambio a WAL no espera al otro proceso ("database is
/// locked"), y dos procesos que leen la misma versión de esquema intentan la
/// misma migración ("table ... already exists"). En ambos casos el otro ya
/// ha terminado para cuando se reintenta.
fn abrir_una_vez(path: &std::path::Path) -> StoreResult<Connection> {
    let mut conn = Connection::open(path)?;
    apply_pragmas(&conn)?;
    run_migrations(&mut conn)?;
    Ok(conn)
}

/// Solo lectura y sin migraciones: no escribe ni crea nada. Rechaza un
/// esquema ajeno ([`StoreError::EsquemaAntiguo`]) sin tocarlo.
pub fn abrir_lectura(path: &std::path::Path) -> StoreResult<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    comprobar_esquema(&conn)?;
    Ok(conn)
}

fn apply_pragmas(conn: &Connection) -> StoreResult<()> {
    // Varios procesos (CLI, ea-mcp, ea-orchestrator) abren esta
    // misma base de datos de forma concurrente; sin esto, dos aperturas
    // simultáneas durante una migración (p. ej. el sondeo y el heartbeat
    // de ea-orchestrator arrancando a la vez) fallan con "database is
    // locked" en vez de esperar el instante que hace falta.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}
