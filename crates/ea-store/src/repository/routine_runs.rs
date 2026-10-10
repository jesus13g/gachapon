use rusqlite::{params, Connection, Row};

use crate::error::{StoreError, StoreResult};
use crate::models::{RoutineRun, RunOutcome};

use super::{now_str, parse_ts};

type RawRoutineRun = (
    i64,
    String,
    Option<i64>,
    String,
    Option<String>,
    Option<String>,
    i64,
    i64,
    Option<String>,
);

fn map_row(row: &Row) -> rusqlite::Result<RawRoutineRun> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

fn to_routine_run(raw: RawRoutineRun) -> StoreResult<RoutineRun> {
    let (
        id,
        routine_slug,
        task_id,
        started_at,
        finished_at,
        outcome,
        pasos_ejecutados,
        llamadas_modelo,
        detalle,
    ) = raw;
    Ok(RoutineRun {
        id,
        routine_slug,
        task_id,
        started_at: parse_ts(&started_at)?,
        finished_at: finished_at.as_deref().map(parse_ts).transpose()?,
        outcome: outcome.map(|o| o.parse::<RunOutcome>()).transpose()?,
        pasos_ejecutados,
        llamadas_modelo,
        detalle,
    })
}

const SELECT_COLUMNS: &str = "id, routine_slug, task_id, started_at, finished_at, outcome, pasos_ejecutados, llamadas_modelo, detalle";

/// Abre una ejecución de rutina. Sin `outcome` todavía: se cierra con
/// [`finish_routine_run`] cuando el ejecutor termina el grafo de pasos
/// (por éxito, presupuesto agotado o error).
pub fn start_routine_run(
    conn: &Connection,
    routine_slug: &str,
    task_id: Option<i64>,
) -> StoreResult<RoutineRun> {
    let now = now_str()?;
    conn.execute(
        "INSERT INTO routine_runs (routine_slug, task_id, started_at, pasos_ejecutados, llamadas_modelo)
         VALUES (?1, ?2, ?3, 0, 0)",
        params![routine_slug, task_id, now],
    )?;
    get_routine_run(conn, conn.last_insert_rowid())
}

/// Cierra una ejecución de rutina con su resultado final.
pub fn finish_routine_run(
    conn: &Connection,
    id: i64,
    outcome: RunOutcome,
    pasos_ejecutados: i64,
    llamadas_modelo: i64,
    detalle: Option<String>,
) -> StoreResult<RoutineRun> {
    let now = now_str()?;
    conn.execute(
        "UPDATE routine_runs
         SET finished_at = ?1, outcome = ?2, pasos_ejecutados = ?3, llamadas_modelo = ?4, detalle = ?5
         WHERE id = ?6",
        params![now, outcome.as_str(), pasos_ejecutados, llamadas_modelo, detalle, id],
    )?;
    get_routine_run(conn, id)
}

pub fn get_routine_run(conn: &Connection, id: i64) -> StoreResult<RoutineRun> {
    let raw = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM routine_runs WHERE id = ?1"),
            params![id],
            map_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::RoutineRunNotFound(id),
            other => StoreError::Database(other),
        })?;
    to_routine_run(raw)
}

/// Última ejecución completada con éxito de una rutina (§10.2: es la que
/// determina la ventana real transcurrida, no la nominal).
pub fn last_successful_run(conn: &Connection, routine_slug: &str) -> StoreResult<Option<RoutineRun>> {
    let raw = conn
        .query_row(
            &format!(
                "SELECT {SELECT_COLUMNS} FROM routine_runs \
                 WHERE routine_slug = ?1 AND outcome = 'ok' \
                 ORDER BY started_at DESC LIMIT 1"
            ),
            params![routine_slug],
            map_row,
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(StoreError::Database(other)),
        })?;
    raw.map(to_routine_run).transpose()
}

pub fn list_routine_runs(
    conn: &Connection,
    routine_slug: &str,
    limit: Option<i64>,
) -> StoreResult<Vec<RoutineRun>> {
    let mut sql = format!("SELECT {SELECT_COLUMNS} FROM routine_runs WHERE routine_slug = ?1 ORDER BY started_at DESC");
    if limit.is_some() {
        sql.push_str(" LIMIT ?2");
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(limit) = limit {
        stmt.query_map(params![routine_slug, limit], map_row)?
    } else {
        stmt.query_map(params![routine_slug], map_row)?
    };

    let mut runs = Vec::new();
    for row in rows {
        runs.push(to_routine_run(row?)?);
    }
    Ok(runs)
}
