use rusqlite::{params, Connection, Row};

use crate::error::{StoreError, StoreResult};
use crate::models::{EstadoPropuesta, Propuesta};

use super::{now_str, parse_ts};

type RawPropuesta = (i64, i64, String, String, String, Option<String>);

fn map_row(row: &Row) -> rusqlite::Result<RawPropuesta> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn to_propuesta(raw: RawPropuesta) -> StoreResult<Propuesta> {
    let (id, routine_run_id, creado_en, descripcion, estado, decidido_en) = raw;
    Ok(Propuesta {
        id,
        routine_run_id,
        creado_en: parse_ts(&creado_en)?,
        descripcion,
        estado: estado.parse::<EstadoPropuesta>()?,
        decidido_en: decidido_en.as_deref().map(parse_ts).transpose()?,
    })
}

const SELECT_COLUMNS: &str = "id, routine_run_id, creado_en, descripcion, estado, decidido_en";

/// Encola una propuesta: la acción mutante que una rutina desatendida no
/// ejecuta (§11.2), a la espera de que el usuario decida al volver.
pub fn create_propuesta(
    conn: &Connection,
    routine_run_id: i64,
    descripcion: &str,
) -> StoreResult<Propuesta> {
    let now = now_str()?;
    conn.execute(
        "INSERT INTO propuestas (routine_run_id, creado_en, descripcion, estado)
         VALUES (?1, ?2, ?3, 'pendiente')",
        params![routine_run_id, now, descripcion],
    )?;
    get_propuesta(conn, conn.last_insert_rowid())
}

pub fn get_propuesta(conn: &Connection, id: i64) -> StoreResult<Propuesta> {
    let raw = conn
        .query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM propuestas WHERE id = ?1"),
            params![id],
            map_row,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StoreError::PropuestaNotFound(id),
            other => StoreError::Database(other),
        })?;
    to_propuesta(raw)
}

#[derive(Debug, Default, Clone)]
pub struct PropuestaFilter {
    pub estado: Option<EstadoPropuesta>,
    pub limit: Option<i64>,
}

pub fn list_propuestas(conn: &Connection, filter: &PropuestaFilter) -> StoreResult<Vec<Propuesta>> {
    let mut sql = format!("SELECT {SELECT_COLUMNS} FROM propuestas WHERE 1 = 1");
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(estado) = filter.estado {
        sql.push_str(" AND estado = ?");
        sql_params.push(Box::new(estado.as_str()));
    }
    sql.push_str(" ORDER BY creado_en DESC");
    if let Some(limit) = filter.limit {
        sql.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit));
    }

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(refs.as_slice(), map_row)?;

    let mut propuestas = Vec::new();
    for row in rows {
        propuestas.push(to_propuesta(row?)?);
    }
    Ok(propuestas)
}

/// Registra la decisión del usuario sobre una propuesta pendiente. No crea
/// ninguna tarea: ejecutar una propuesta aceptada requiere una herramienta
/// mutante, que no existe todavía (fase 6) — esto solo deja constancia de
/// la decisión.
pub fn decidir_propuesta(
    conn: &Connection,
    id: i64,
    estado: EstadoPropuesta,
) -> StoreResult<Propuesta> {
    if estado == EstadoPropuesta::Pendiente {
        return Err(StoreError::InvalidStatus(estado.as_str().to_string()));
    }
    let now = now_str()?;
    conn.execute(
        "UPDATE propuestas SET estado = ?1, decidido_en = ?2 WHERE id = ?3",
        params![estado.as_str(), now, id],
    )?;
    get_propuesta(conn, id)
}
