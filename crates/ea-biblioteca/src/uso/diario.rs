//! Diario del ecosistema (SQLite, tabla `events`): los `mcp_*` son llamadas
//! al servidor `ea-mcp`. Se lee con `ea-store` en solo lectura
//! ([`Store::abrir_lectura`]): sin migraciones y sin poder escribir ni crear
//! nada ahí; una base de un esquema ajeno se rechaza sin tocarla.
//!
//! El actor es `detail.agente` si existe y, si no, la columna `actor`.

use std::path::Path;

use ea_store::{Store, StoreError};

/// `(actor, usos, último en ms)` de las acciones `mcp_*`.
pub fn leer(ruta: &Path, avisos: &mut Vec<String>) -> Vec<(String, u64, Option<i64>)> {
    if !ruta.is_file() {
        return Vec::new();
    }
    match Store::abrir_lectura(ruta).and_then(|s| s.uso_mcp_por_actor()) {
        Ok(v) => v.into_iter().map(|(a, n, t)| (a, n, t.map(|d| (d.unix_timestamp_nanos() / 1_000_000) as i64))).collect(),
        Err(e) => {
            // El detalle de SQLite tal cual, sin el prefijo del error del store.
            let detalle = match e {
                StoreError::Database(e) => e.to_string(),
                e => e.to_string(),
            };
            avisos.push(format!("{}: no se pudo leer el diario ({detalle})", ruta.display()));
            Vec::new()
        }
    }
}
