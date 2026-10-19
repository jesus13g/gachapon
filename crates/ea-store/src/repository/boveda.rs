//! Índice de búsqueda de la bóveda (migración 0004): una fila por nota y su
//! entrada en la tabla FTS5. Es solo una caché derivada de los ficheros; la
//! escritura de las notas y su semántica viven en `ea-boveda`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::StoreResult;
use crate::models::{EntradaIndice, ResultadoBusqueda, RutaIndexada};

/// Inserta o reemplaza la nota en el índice (y su texto en FTS5), de forma
/// atómica.
pub fn indexar(conn: &mut Connection, e: &EntradaIndice, cuerpo: &str) -> StoreResult<()> {
    let tx = conn.transaction()?;
    let previo: Option<i64> =
        tx.query_row("SELECT id FROM boveda_notas WHERE ruta = ?1", [&e.ruta], |r| r.get(0)).optional()?;
    if let Some(id) = previo {
        tx.execute("DELETE FROM boveda_fts WHERE rowid = ?1", [id])?;
        tx.execute("DELETE FROM boveda_notas WHERE id = ?1", [id])?;
    }
    tx.execute(
        "INSERT INTO boveda_notas
            (ruta, region, autor, titulo, origen, promocion, creada, actualizada, mtime_ns, bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            e.ruta, e.region, e.autor, e.titulo, e.origen, e.promocion, e.creada, e.actualizada,
            e.mtime_ns, e.bytes
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO boveda_fts (rowid, titulo, tags, cuerpo) VALUES (?1, ?2, ?3, ?4)",
        params![id, e.titulo, e.tags, cuerpo],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn olvidar(conn: &mut Connection, ruta: &str) -> StoreResult<()> {
    let tx = conn.transaction()?;
    let id: Option<i64> =
        tx.query_row("SELECT id FROM boveda_notas WHERE ruta = ?1", [ruta], |r| r.get(0)).optional()?;
    if let Some(id) = id {
        tx.execute("DELETE FROM boveda_fts WHERE rowid = ?1", [id])?;
        tx.execute("DELETE FROM boveda_notas WHERE id = ?1", [id])?;
    }
    tx.commit()?;
    Ok(())
}

/// Lo que el índice sabe de cada nota, para detectar cambios externos.
pub fn rutas_indexadas(conn: &Connection) -> StoreResult<Vec<RutaIndexada>> {
    let mut st = conn.prepare("SELECT ruta, mtime_ns, bytes FROM boveda_notas")?;
    let filas = st.query_map([], |r| {
        Ok(RutaIndexada { ruta: r.get(0)?, mtime_ns: r.get(1)?, bytes: r.get(2)? })
    })?;
    Ok(filas.collect::<Result<_, _>>()?)
}

/// Busca con FTS5. `consulta_fts` ya viene saneada por quien llama (cada
/// término entrecomillado): aquí no se interpreta sintaxis de usuario.
pub fn buscar(
    conn: &Connection,
    consulta_fts: &str,
    region: Option<&str>,
    limite: i64,
    max_fragmento: i64,
) -> StoreResult<Vec<ResultadoBusqueda>> {
    // `snippet` cubre la columna `cuerpo` (índice 2) y acota por número de
    // tokens; el recorte final en caracteres lo hace quien llama.
    let mut st = conn.prepare(
        "SELECT n.ruta, n.region, n.autor, n.titulo, n.actualizada, n.promocion,
                snippet(boveda_fts, 2, '«', '»', '…', ?4)
           FROM boveda_fts
           JOIN boveda_notas n ON n.id = boveda_fts.rowid
          WHERE boveda_fts MATCH ?1 AND (?2 IS NULL OR n.region = ?2)
          ORDER BY bm25(boveda_fts, 6.0, 3.0, 1.0)
          LIMIT ?3",
    )?;
    let filas = st.query_map(params![consulta_fts, region, limite, max_fragmento], |r| {
        Ok(ResultadoBusqueda {
            ruta: r.get(0)?,
            region: r.get(1)?,
            autor: r.get(2)?,
            titulo: r.get(3)?,
            actualizada: r.get(4)?,
            promocion: r.get(5)?,
            fragmento: r.get(6)?,
        })
    })?;
    Ok(filas.collect::<Result<_, _>>()?)
}

/// Notas más recientemente actualizadas, sin consulta.
pub fn recientes(conn: &Connection, region: Option<&str>, limite: i64) -> StoreResult<Vec<ResultadoBusqueda>> {
    let mut st = conn.prepare(
        "SELECT ruta, region, autor, titulo, actualizada, promocion
           FROM boveda_notas
          WHERE (?1 IS NULL OR region = ?1)
          ORDER BY COALESCE(actualizada, '') DESC, id DESC
          LIMIT ?2",
    )?;
    let filas = st.query_map(params![region, limite], |r| {
        Ok(ResultadoBusqueda {
            ruta: r.get(0)?,
            region: r.get(1)?,
            autor: r.get(2)?,
            titulo: r.get(3)?,
            actualizada: r.get(4)?,
            promocion: r.get(5)?,
            fragmento: String::new(),
        })
    })?;
    Ok(filas.collect::<Result<_, _>>()?)
}

/// Todas las notas del índice, por ruta: el árbol de la bóveda.
pub fn todas(conn: &Connection) -> StoreResult<Vec<ResultadoBusqueda>> {
    let mut st = conn.prepare(
        "SELECT ruta, region, autor, titulo, actualizada, promocion
           FROM boveda_notas
          ORDER BY ruta",
    )?;
    let filas = st.query_map([], |r| {
        Ok(ResultadoBusqueda {
            ruta: r.get(0)?,
            region: r.get(1)?,
            autor: r.get(2)?,
            titulo: r.get(3)?,
            actualizada: r.get(4)?,
            promocion: r.get(5)?,
            fragmento: String::new(),
        })
    })?;
    Ok(filas.collect::<Result<_, _>>()?)
}

pub fn contar_por_region(conn: &Connection) -> StoreResult<Vec<(String, i64)>> {
    let mut st = conn.prepare("SELECT region, COUNT(*) FROM boveda_notas GROUP BY region ORDER BY region")?;
    let filas = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(filas.collect::<Result<_, _>>()?)
}
