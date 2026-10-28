use rusqlite::{params, Connection, Row};

use crate::error::{StoreError, StoreResult};
use crate::models::{Event, EventOutcome, NewEvent};

use super::{fmt_ts, parse_ts};

/// Una fila de `events` tal como sale de SQLite: id, ts, actor, action,
/// trabajo, outcome, duration_ms y detail.
type FilaEvento = (i64, String, String, String, Option<String>, Option<String>, Option<i64>, Option<String>);

fn map_row(row: &Row) -> rusqlite::Result<FilaEvento> {
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

fn to_event(raw: FilaEvento) -> StoreResult<Event> {
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
    let ts = crate::ahora_rfc3339();
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
    /// Solo los eventos con este resultado (`ok` o `error`).
    pub outcome: Option<EventOutcome>,
    /// Solo las acciones que empiezan por este prefijo (p. ej. `bus_` o
    /// `propuesta_`), una familia de eventos. Los comodines de `LIKE` del
    /// prefijo se escapan: se compara literalmente.
    pub action_prefix: Option<String>,
    /// Cursor de paginación: solo los eventos que van *después* del evento
    /// con este id en el orden del listado (`ts DESC, id DESC`), es decir, los
    /// más antiguos que él. Estable aunque lleguen eventos nuevos. Un id que
    /// no existe no devuelve nada.
    pub before_id: Option<i64>,
    pub limit: Option<i64>,
}

/// Escapa `%`, `_` y la propia barra para usar `texto` como literal en un
/// `LIKE ... ESCAPE '\'`.
fn escapar_like(texto: &str) -> String {
    let mut s = String::with_capacity(texto.len());
    for c in texto.chars() {
        if matches!(c, '%' | '_' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s
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
    if let Some(outcome) = filter.outcome {
        sql.push_str(" AND outcome = ?");
        sql_params.push(Box::new(outcome.as_str()));
    }
    if let Some(prefijo) = &filter.action_prefix {
        sql.push_str(" AND action LIKE ? || '%' ESCAPE '\\'");
        sql_params.push(Box::new(escapar_like(prefijo)));
    }
    if let Some(id) = filter.before_id {
        sql.push_str(" AND (ts, id) < (SELECT ts, id FROM events WHERE id = ?)");
        sql_params.push(Box::new(id));
    }
    // `id` desempata los eventos del mismo instante: el orden es total y el
    // cursor `before_id` no salta ni repite filas.
    sql.push_str(" ORDER BY ts DESC, id DESC");
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

/// Los eventos de una herramienta del catálogo (`ea_<x>`), del más reciente
/// al más antiguo: sus invocaciones por MCP (`mcp_<x>`, `mcp_<x>_simulada`,
/// `mcp_<x>_denegada`) y los `accion_*` y `propuesta_*` cuyo `detail` la
/// nombra en `herramienta` o apunta a una propuesta suya (el rechazo de una
/// propuesta solo trae su id). Sin tocar el esquema: el `detail` se lee con
/// `json_extract`, y solo cuando es JSON válido.
pub fn list_events_herramienta(conn: &Connection, herramienta: &str, limit: i64) -> StoreResult<Vec<Event>> {
    let mcp = format!("mcp_{}", herramienta.strip_prefix("ea_").unwrap_or(herramienta));
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM events
         WHERE action IN (?1, ?1 || '_simulada', ?1 || '_denegada')
            OR ((action LIKE 'accion\\_%' ESCAPE '\\' OR action LIKE 'propuesta\\_%' ESCAPE '\\')
                AND CASE WHEN json_valid(detail) THEN
                      json_extract(detail, '$.herramienta') = ?2
                      OR json_extract(detail, '$.propuesta') IN
                         (SELECT id FROM propuestas WHERE accion_herramienta = ?2)
                    ELSE 0 END)
         ORDER BY ts DESC, id DESC
         LIMIT ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![mcp, herramienta, limit], map_row)?;
    let mut events = Vec::new();
    for row in rows {
        events.push(to_event(row?)?);
    }
    Ok(events)
}

/// `(actor, usos, última)` de las llamadas al servidor `ea-mcp` (acciones
/// `mcp_*`), por actor: `detail.agente` si existe y, si no, la columna
/// `actor`. Una fecha ilegible cuenta como desconocida, no como error.
pub fn uso_mcp_por_actor(conn: &Connection) -> StoreResult<Vec<(String, u64, Option<time::OffsetDateTime>)>> {
    let mut st = conn.prepare(
        "SELECT coalesce(CASE WHEN json_valid(detail) THEN json_extract(detail, '$.agente') END, actor) AS a,
                count(*), max(ts)
         FROM events WHERE action LIKE 'mcp\\_%' ESCAPE '\\' GROUP BY a",
    )?;
    let filas = st.query_map([], |f| Ok((f.get::<_, String>(0)?, f.get::<_, i64>(1)? as u64, f.get::<_, Option<String>>(2)?)))?;
    let mut uso = Vec::new();
    for fila in filas {
        let (actor, n, ts) = fila?;
        uso.push((actor, n, ts.as_deref().and_then(|t| parse_ts(t).ok())));
    }
    Ok(uso)
}
