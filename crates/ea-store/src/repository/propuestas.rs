use rusqlite::{Connection, Row, params};

use crate::error::{StoreError, StoreResult};
use crate::models::{AccionPropuesta, EjecucionPropuesta, EstadoEjecucion, EstadoPropuesta, NuevaPropuesta, Propuesta};

use super::parse_ts;

const SELECT_COLUMNS: &str = "id, creado_en, descripcion, estado, decidido_en, \
    proponente, accion_herramienta, accion_argumentos, accion_riesgo, accion_simulacion, \
    accion_precondicion, ejecucion_estado, ejecucion_detalle, ejecutada_en";

struct Raw {
    id: i64,
    creado_en: String,
    descripcion: String,
    estado: String,
    decidido_en: Option<String>,
    proponente: Option<String>,
    herramienta: Option<String>,
    argumentos: Option<String>,
    riesgo: Option<String>,
    simulacion: Option<String>,
    precondicion: Option<String>,
    ejecucion_estado: Option<String>,
    ejecucion_detalle: Option<String>,
    ejecutada_en: Option<String>,
}

fn map_row(row: &Row) -> rusqlite::Result<Raw> {
    Ok(Raw {
        id: row.get(0)?,
        creado_en: row.get(1)?,
        descripcion: row.get(2)?,
        estado: row.get(3)?,
        decidido_en: row.get(4)?,
        proponente: row.get(5)?,
        herramienta: row.get(6)?,
        argumentos: row.get(7)?,
        riesgo: row.get(8)?,
        simulacion: row.get(9)?,
        precondicion: row.get(10)?,
        ejecucion_estado: row.get(11)?,
        ejecucion_detalle: row.get(12)?,
        ejecutada_en: row.get(13)?,
    })
}

fn json_o_nulo(texto: Option<String>) -> serde_json::Value {
    texto.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::Value::Null)
}

fn to_propuesta(raw: Raw) -> StoreResult<Propuesta> {
    let accion = match (raw.herramienta, raw.riesgo) {
        (Some(herramienta), Some(riesgo)) => Some(AccionPropuesta {
            herramienta,
            argumentos: json_o_nulo(raw.argumentos),
            riesgo,
            simulacion: raw.simulacion.unwrap_or_default(),
            precondicion: json_o_nulo(raw.precondicion),
        }),
        _ => None,
    };
    let ejecucion = match raw.ejecucion_estado {
        Some(estado) => Some(EjecucionPropuesta {
            estado: estado.parse::<EstadoEjecucion>()?,
            detalle: raw.ejecucion_detalle,
            ejecutada_en: raw.ejecutada_en.as_deref().map(parse_ts).transpose()?,
        }),
        None => None,
    };
    Ok(Propuesta {
        id: raw.id,
        proponente: raw.proponente,
        creado_en: parse_ts(&raw.creado_en)?,
        descripcion: raw.descripcion,
        estado: raw.estado.parse::<EstadoPropuesta>()?,
        decidido_en: raw.decidido_en.as_deref().map(parse_ts).transpose()?,
        accion,
        ejecucion,
    })
}

/// Encola una propuesta, con o sin acción estructurada.
pub fn crear(conn: &Connection, nueva: NuevaPropuesta) -> StoreResult<Propuesta> {
    let now = crate::ahora_rfc3339();
    let (herramienta, argumentos, riesgo, simulacion, precondicion) = match &nueva.accion {
        Some(a) => {
            (Some(a.herramienta.clone()), Some(a.argumentos.to_string()), Some(a.riesgo.clone()), Some(a.simulacion.clone()), Some(a.precondicion.to_string()))
        }
        None => (None, None, None, None, None),
    };
    conn.execute(
        "INSERT INTO propuestas (creado_en, descripcion, estado, proponente,
            accion_herramienta, accion_argumentos, accion_riesgo, accion_simulacion, accion_precondicion)
         VALUES (?1, ?2, 'pendiente', ?3, ?4, ?5, ?6, ?7, ?8)",
        params![now, nueva.descripcion, nueva.proponente, herramienta, argumentos, riesgo, simulacion, precondicion],
    )?;
    get_propuesta(conn, conn.last_insert_rowid())
}

pub fn get_propuesta(conn: &Connection, id: i64) -> StoreResult<Propuesta> {
    let raw = conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM propuestas WHERE id = ?1"), params![id], map_row).map_err(|e| match e {
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
    sql.push_str(" ORDER BY creado_en DESC, id DESC");
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

/// Registra la decisión del usuario sobre una propuesta pendiente de texto
/// libre. Solo transita desde `pendiente` (decidir dos veces es un error) y
/// una propuesta con acción solo puede rechazarse por aquí: aceptarla es
/// ejecutarla, y eso pasa por [`reclamar_para_ejecucion`].
pub fn decidir_propuesta(conn: &Connection, id: i64, estado: EstadoPropuesta) -> StoreResult<Propuesta> {
    if estado == EstadoPropuesta::Pendiente {
        return Err(StoreError::InvalidStatus(estado.as_str().to_string()));
    }
    let actual = get_propuesta(conn, id)?;
    if estado == EstadoPropuesta::Aceptada && actual.accion.is_some() {
        return Err(StoreError::PropuestaConAccion(id));
    }
    let now = crate::ahora_rfc3339();
    let filas =
        conn.execute("UPDATE propuestas SET estado = ?1, decidido_en = ?2 WHERE id = ?3 AND estado = 'pendiente'", params![estado.as_str(), now, id])?;
    if filas == 0 {
        return Err(StoreError::PropuestaYaDecidida(id));
    }
    get_propuesta(conn, id)
}

/// Acepta una propuesta con acción y la marca `ejecutando`, en una sola
/// sentencia condicionada a `estado = 'pendiente'`: de dos aceptaciones
/// concurrentes solo una obtiene la propuesta; la otra recibe
/// `PropuestaYaDecidida`. Es lo que hace idempotente la aceptación.
pub fn reclamar_para_ejecucion(conn: &Connection, id: i64) -> StoreResult<Propuesta> {
    let actual = get_propuesta(conn, id)?;
    if actual.accion.is_none() {
        return Err(StoreError::PropuestaSinAccion(id));
    }
    let now = crate::ahora_rfc3339();
    let filas = conn.execute(
        "UPDATE propuestas SET estado = 'aceptada', decidido_en = ?1, ejecucion_estado = 'ejecutando'
         WHERE id = ?2 AND estado = 'pendiente'",
        params![now, id],
    )?;
    if filas == 0 {
        return Err(StoreError::PropuestaYaDecidida(id));
    }
    get_propuesta(conn, id)
}

/// Cierra la ejecución reclamada con su resultado. Solo actúa sobre una
/// propuesta `ejecutando`: no se puede reescribir un resultado ya registrado.
pub fn registrar_ejecucion(conn: &Connection, id: i64, estado: EstadoEjecucion, detalle: &str) -> StoreResult<Propuesta> {
    if estado == EstadoEjecucion::Ejecutando {
        return Err(StoreError::InvalidStatus(estado.as_str().to_string()));
    }
    let now = crate::ahora_rfc3339();
    let filas = conn.execute(
        "UPDATE propuestas SET ejecucion_estado = ?1, ejecucion_detalle = ?2, ejecutada_en = ?3
         WHERE id = ?4 AND ejecucion_estado = 'ejecutando'",
        params![estado.as_str(), detalle, now, id],
    )?;
    if filas == 0 {
        return Err(StoreError::PropuestaYaDecidida(id));
    }
    get_propuesta(conn, id)
}
