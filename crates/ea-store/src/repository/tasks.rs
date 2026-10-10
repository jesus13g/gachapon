use rusqlite::{params, Connection, Row};
use time::OffsetDateTime;

use crate::error::{StoreError, StoreResult};
use crate::models::{EventOutcome, NewTask, Task, TaskOrigin, TaskStatus};

use super::{fmt_ts, now_str, parse_ts};

type RawTask = (
    i64,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
    String,
    String,
    String,
    Option<String>,
);

fn map_row(row: &Row) -> rusqlite::Result<RawTask> {
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
        row.get(9)?,
        row.get(10)?,
    ))
}

fn to_task(raw: RawTask) -> StoreResult<Task> {
    let (
        id,
        created_at,
        updated_at,
        created_by,
        assigned_to,
        origin,
        parent_task_id,
        title,
        request,
        status,
        result,
    ) = raw;
    Ok(Task {
        id,
        created_at: parse_ts(&created_at)?,
        updated_at: parse_ts(&updated_at)?,
        created_by,
        assigned_to,
        origin: origin.parse::<TaskOrigin>()?,
        parent_task_id,
        title,
        request,
        status: status.parse::<TaskStatus>()?,
        result,
    })
}

const SELECT_COLUMNS: &str = "id, created_at, updated_at, created_by, assigned_to, origin, parent_task_id, title, request, status, result";

/// Acción del evento emitido en el diario para cada transición de estado.
fn action_for_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Pendiente => "tarea_pendiente",
        TaskStatus::EnProgreso => "tarea_iniciada",
        TaskStatus::Completada => "tarea_completada",
        TaskStatus::Fallida => "tarea_fallida",
        TaskStatus::Cancelada => "tarea_cancelada",
    }
}

fn outcome_for_status(status: TaskStatus) -> Option<EventOutcome> {
    match status {
        TaskStatus::Completada | TaskStatus::Cancelada => Some(EventOutcome::Ok),
        TaskStatus::Fallida => Some(EventOutcome::Error),
        TaskStatus::Pendiente | TaskStatus::EnProgreso => None,
    }
}

/// Crea una tarea y registra su evento de creación en una sola transacción.
/// Es el mismo camino que recorrerá, en fases futuras, un botón de interfaz,
/// una orden de voz o un disparador programado (§9.1: primitiva única).
pub fn create_task(conn: &mut Connection, new_task: NewTask) -> StoreResult<Task> {
    let now = now_str()?;
    let tx = conn.transaction()?;

    tx.execute(
        "INSERT INTO tasks (created_at, updated_at, created_by, assigned_to, origin, parent_task_id, title, request, status, result)
         VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pendiente', NULL)",
        params![
            now,
            new_task.created_by,
            new_task.assigned_to,
            new_task.origin.as_str(),
            new_task.parent_task_id,
            new_task.title,
            new_task.request,
        ],
    )?;
    let task_id = tx.last_insert_rowid();

    tx.execute(
        "INSERT INTO events (ts, actor, action, task_id, outcome, duration_ms, detail)
         VALUES (?1, ?2, 'tarea_creada', ?3, 'ok', NULL, NULL)",
        params![now, new_task.created_by, task_id],
    )?;

    tx.commit()?;
    get_task(conn, task_id)
}

/// Actualiza el estado de una tarea y registra el evento correspondiente,
/// en una sola transacción. Si el nuevo estado es terminal, calcula
/// `duration_ms` desde la creación de la tarea.
pub fn update_status(
    conn: &mut Connection,
    task_id: i64,
    new_status: TaskStatus,
    actor: &str,
    result: Option<String>,
) -> StoreResult<Task> {
    let tx = conn.transaction()?;

    let created_at_str: String = tx.query_row(
        "SELECT created_at FROM tasks WHERE id = ?1",
        params![task_id],
        |row| row.get(0),
    ).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => StoreError::TaskNotFound(task_id),
        other => StoreError::Database(other),
    })?;

    let now = OffsetDateTime::now_utc();
    let now_str = fmt_ts(now)?;

    let duration_ms = if new_status.is_terminal() {
        let created_at = parse_ts(&created_at_str)?;
        Some((now - created_at).whole_milliseconds() as i64)
    } else {
        None
    };

    tx.execute(
        "UPDATE tasks SET status = ?1, result = ?2, updated_at = ?3 WHERE id = ?4",
        params![new_status.as_str(), result, now_str, task_id],
    )?;

    tx.execute(
        "INSERT INTO events (ts, actor, action, task_id, outcome, duration_ms, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
        params![
            now_str,
            actor,
            action_for_status(new_status),
            task_id,
            outcome_for_status(new_status).map(|o| o.as_str()),
            duration_ms,
        ],
    )?;

    tx.commit()?;
    get_task(conn, task_id)
}

pub fn get_task(conn: &Connection, id: i64) -> StoreResult<Task> {
    let raw = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM tasks WHERE id = ?1"),
            params![id],
            map_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::TaskNotFound(id),
            other => StoreError::Database(other),
        })?;
    to_task(raw)
}

#[derive(Debug, Default, Clone)]
pub struct TaskFilter {
    pub status: Option<TaskStatus>,
    pub assigned_to: Option<String>,
    pub limit: Option<i64>,
}

pub fn list_tasks(conn: &Connection, filter: &TaskFilter) -> StoreResult<Vec<Task>> {
    let mut sql = format!("SELECT {SELECT_COLUMNS} FROM tasks WHERE 1 = 1");
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(status) = filter.status {
        sql.push_str(" AND status = ?");
        sql_params.push(Box::new(status.as_str()));
    }
    if let Some(assigned_to) = &filter.assigned_to {
        sql.push_str(" AND assigned_to = ?");
        sql_params.push(Box::new(assigned_to.clone()));
    }
    sql.push_str(" ORDER BY created_at DESC");
    if let Some(limit) = filter.limit {
        sql.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit));
    }

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_row)?;

    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(to_task(row?)?);
    }
    Ok(tasks)
}
