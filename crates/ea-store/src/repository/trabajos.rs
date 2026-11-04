//! Historial de trabajos del orquestador (migración 0005). Los ficheros del
//! bus son la fuente de verdad; esto es el registro consultable.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StoreResult;

/// Fila de `trabajos`. Las marcas de tiempo son texto RFC 3339.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilaTrabajo {
    pub id: String,
    pub modo: String,
    pub titulo: String,
    pub worktree: String,
    pub estado: String,
    pub creado_en: String,
    pub actualizado_en: String,
    pub creado_por: String,
    pub rutina_slug: Option<String>,
    pub rutina_version: Option<i64>,
}

/// Fila de `trabajo_fases`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilaFase {
    pub trabajo_id: String,
    pub numero: i64,
    pub titulo: String,
    pub estado: String,
    pub informe: Option<String>,
    pub cerrado_en: Option<String>,
}

/// Fila de `rutinas`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilaRutina {
    pub slug: String,
    pub version: i64,
    pub definicion: String,
    pub creada_en: String,
    pub creada_por: String,
}

pub fn insertar_trabajo(conn: &Connection, t: &FilaTrabajo) -> StoreResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO trabajos (id, modo, titulo, worktree, estado, creado_en, actualizado_en, creado_por, rutina_slug, rutina_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![t.id, t.modo, t.titulo, t.worktree, t.estado, t.creado_en, t.actualizado_en, t.creado_por, t.rutina_slug, t.rutina_version],
    )?;
    Ok(())
}

pub fn actualizar_estado_trabajo(conn: &Connection, id: &str, estado: &str, actualizado_en: &str) -> StoreResult<()> {
    conn.execute("UPDATE trabajos SET estado = ?2, actualizado_en = ?3 WHERE id = ?1", params![id, estado, actualizado_en])?;
    Ok(())
}

pub fn obtener_trabajo(conn: &Connection, id: &str) -> StoreResult<Option<FilaTrabajo>> {
    Ok(conn
        .query_row(
            "SELECT id, modo, titulo, worktree, estado, creado_en, actualizado_en, creado_por, rutina_slug, rutina_version
             FROM trabajos WHERE id = ?1",
            params![id],
            |r| {
                Ok(FilaTrabajo {
                    id: r.get(0)?,
                    modo: r.get(1)?,
                    titulo: r.get(2)?,
                    worktree: r.get(3)?,
                    estado: r.get(4)?,
                    creado_en: r.get(5)?,
                    actualizado_en: r.get(6)?,
                    creado_por: r.get(7)?,
                    rutina_slug: r.get(8)?,
                    rutina_version: r.get(9)?,
                })
            },
        )
        .optional()?)
}

pub fn guardar_fase(conn: &Connection, f: &FilaFase) -> StoreResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO trabajo_fases (trabajo_id, numero, titulo, estado, informe, cerrado_en)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![f.trabajo_id, f.numero, f.titulo, f.estado, f.informe, f.cerrado_en],
    )?;
    Ok(())
}

pub fn listar_fases(conn: &Connection, trabajo_id: &str) -> StoreResult<Vec<FilaFase>> {
    let mut st = conn.prepare("SELECT trabajo_id, numero, titulo, estado, informe, cerrado_en FROM trabajo_fases WHERE trabajo_id = ?1 ORDER BY numero")?;
    let filas = st
        .query_map(params![trabajo_id], |r| {
            Ok(FilaFase { trabajo_id: r.get(0)?, numero: r.get(1)?, titulo: r.get(2)?, estado: r.get(3)?, informe: r.get(4)?, cerrado_en: r.get(5)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(filas)
}

pub fn guardar_rutina(conn: &Connection, r: &FilaRutina) -> StoreResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO rutinas (slug, version, definicion, creada_en, creada_por) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![r.slug, r.version, r.definicion, r.creada_en, r.creada_por],
    )?;
    Ok(())
}

pub fn obtener_rutina(conn: &Connection, slug: &str, version: i64) -> StoreResult<Option<FilaRutina>> {
    Ok(conn
        .query_row("SELECT slug, version, definicion, creada_en, creada_por FROM rutinas WHERE slug = ?1 AND version = ?2", params![slug, version], |r| {
            Ok(FilaRutina { slug: r.get(0)?, version: r.get(1)?, definicion: r.get(2)?, creada_en: r.get(3)?, creada_por: r.get(4)? })
        })
        .optional()?)
}

/// Última versión registrada de una rutina.
pub fn ultima_rutina(conn: &Connection, slug: &str) -> StoreResult<Option<FilaRutina>> {
    Ok(conn
        .query_row("SELECT slug, version, definicion, creada_en, creada_por FROM rutinas WHERE slug = ?1 ORDER BY version DESC LIMIT 1", params![slug], |r| {
            Ok(FilaRutina { slug: r.get(0)?, version: r.get(1)?, definicion: r.get(2)?, creada_en: r.get(3)?, creada_por: r.get(4)? })
        })
        .optional()?)
}

/// Todas las versiones de una rutina, la más reciente primero (vacío si no existe).
pub fn listar_versiones_rutina(conn: &Connection, slug: &str) -> StoreResult<Vec<FilaRutina>> {
    let mut st = conn.prepare("SELECT slug, version, definicion, creada_en, creada_por FROM rutinas WHERE slug = ?1 ORDER BY version DESC")?;
    let filas = st
        .query_map(params![slug], |r| {
            Ok(FilaRutina { slug: r.get(0)?, version: r.get(1)?, definicion: r.get(2)?, creada_en: r.get(3)?, creada_por: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(filas)
}

/// La última versión de cada rutina, ordenadas por slug.
pub fn listar_rutinas(conn: &Connection) -> StoreResult<Vec<FilaRutina>> {
    let mut st = conn.prepare(
        "SELECT slug, version, definicion, creada_en, creada_por FROM rutinas r
         WHERE version = (SELECT MAX(version) FROM rutinas WHERE slug = r.slug) ORDER BY slug",
    )?;
    let filas = st
        .query_map([], |r| Ok(FilaRutina { slug: r.get(0)?, version: r.get(1)?, definicion: r.get(2)?, creada_en: r.get(3)?, creada_por: r.get(4)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(filas)
}
