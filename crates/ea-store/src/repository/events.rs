use rusqlite::{params, Connection, Row};

use crate::error::{StoreError, StoreResult};
use crate::models::{Event, EventOutcome, NewEvent};

use super::{fmt_ts, now_str, parse_ts};

fn map_row(row: &Row) -> rusqlite::Result<(i64, String, String, String, Option<String>, Option<String>, Option<i64>, Option<String>)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn to_event(
    raw: (
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    ),
) -> StoreResult<Event> {
    let (id, ts, actor, action, trabajo, outcome, duration_ms, detail) = raw;
    let outcome = outcome
        .map(|o| o.parse::<EventOutcome>())
        .transpose()?;
    Ok(Event {
        id,
        ts: parse_ts(&ts)?,
        actor,
        action,
        trabajo,
        outcome,
        duration_ms,
        detail,
    })
}

const SELECT_COLUMNS: &str = "id, ts, actor, action, trabajo, outcome, duration_ms, detail";

/// Añade un evento al diario, opcionalmente asociado a un trabajo.
pub fn append_event(conn: &Connection, new_event: NewEvent) -> StoreResult<Event> {
    let ts = now_str()?;
    conn.execute(
        "INSERT INTO events (ts, actor, action, trabajo, outcome, duration_ms, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            ts,
            new_event.actor,
            new_event.action,
            new_event.trabajo,
            new_event.outcome.map(|o| o.as_str()),
            new_event.duration_ms,
            new_event.detail,
        ],
    )?;
    get_event(conn, conn.last_insert_rowid())
}

pub fn get_event(conn: &Connection, id: i64) -> StoreResult<Event> {
    let raw = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM events WHERE id = ?1"),
            params![id],
            map_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::EventNotFound(id),
            other => StoreError::Database(other),
        })?;
    to_event(raw)
}

#[derive(Debug, Default, Clone)]
pub struct EventFilter {
    pub trabajo: Option<String>,
    pub actor: Option<String>,
    pub since: Option<time::OffsetDateTime>,
    pub until: Option<time::OffsetDateTime>,
    /// Excluye las filas con esta acción (p. ej. `heartbeat`, que el
    /// orquestador emite cada pocos segundos y ahogaría cualquier listado
    /// acotado por `limit`).
    pub exclude_action: Option<String>,
    pub limit: Option<i64>,
}

pub fn list_events(conn: &Connection, filter: &EventFilter) -> StoreResult<Vec<Event>> {
    let mut sql = format!("SELECT {SELECT_COLUMNS} FROM events WHERE 1 = 1");
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(trabajo) = &filter.trabajo {
        sql.push_str(" AND trabajo = ?");
        sql_params.push(Box::new(trabajo.clone()));
    }
    if let Some(actor) = &filter.actor {
        sql.push_str(" AND actor = ?");
        sql_params.push(Box::new(actor.clone()));
    }
    if let Some(since) = filter.since {
        sql.push_str(" AND ts >= ?");
        sql_params.push(Box::new(fmt_ts(since)?));
    }
    if let Some(until) = filter.until {
        sql.push_str(" AND ts <= ?");
        sql_params.push(Box::new(fmt_ts(until)?));
    }
    if let Some(exclude_action) = &filter.exclude_action {
        sql.push_str(" AND action != ?");
        sql_params.push(Box::new(exclude_action.clone()));
    }
    sql.push_str(" ORDER BY ts DESC");
    if let Some(limit) = filter.limit {
        sql.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit));
    }

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_row)?;

    let mut events = Vec::new();
    for row in rows {
        events.push(to_event(row?)?);
    }
    Ok(events)
}

pub fn list_events_by_trabajo(conn: &Connection, trabajo: &str) -> StoreResult<Vec<Event>> {
    list_events(
        conn,
        &EventFilter {
            trabajo: Some(trabajo.to_string()),
            ..Default::default()
        },
    )
}
