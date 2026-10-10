use std::path::PathBuf;

use directories::ProjectDirs;
use rusqlite::Connection;

use crate::error::{StoreError, StoreResult};
use crate::migrations::run_migrations;

const ENV_OVERRIDE: &str = "EA_DB_PATH";
const DB_FILE_NAME: &str = "ecosistema.db";

/// Resuelve la ruta del fichero SQLite: `$EA_DB_PATH` si está definida
/// (usado por tests de integración y pruebas manuales), si no la ruta XDG
/// estándar de datos de la aplicación.
pub fn resolve_db_path() -> StoreResult<PathBuf> {
    if let Ok(path) = std::env::var(ENV_OVERRIDE) {
        return Ok(PathBuf::from(path));
    }

    let dirs = ProjectDirs::from("", "", "ecosistema-asistente").ok_or(StoreError::NoDataDir)?;
    let data_dir = dirs.data_dir();
    std::fs::create_dir_all(data_dir)?;
    Ok(data_dir.join(DB_FILE_NAME))
}

/// Abre la base de datos en la ruta resuelta, aplica los PRAGMAs necesarios
/// y ejecuta las migraciones pendientes. Punto de entrada único para todo
/// consumidor de `ea-store` (CLI hoy; orquestador y servidor MCP en fases
/// futuras).
pub fn open() -> StoreResult<Connection> {
    let path = resolve_db_path()?;
    open_at(&path)
}

/// Igual que [`open`] pero sobre una ruta explícita. Útil para tests
/// (`:memory:`) y para herramientas de diagnóstico.
pub fn open_at(path: &std::path::Path) -> StoreResult<Connection> {
    const INTENTOS: u32 = 5;
    let mut intento = 1;
    loop {
        match abrir_una_vez(path) {
            Ok(conn) => return Ok(conn),
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

fn apply_pragmas(conn: &Connection) -> StoreResult<()> {
    // Varios procesos (CLI, ea-mcp, ea-proxy, ea-orchestrator) abren esta
    // misma base de datos de forma concurrente; sin esto, dos aperturas
    // simultáneas durante una migración (p. ej. el sondeo y el heartbeat
    // de ea-orchestrator arrancando a la vez) fallan con "database is
    // locked" en vez de esperar el instante que hace falta.
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}
