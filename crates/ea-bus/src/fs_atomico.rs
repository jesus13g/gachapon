//! Escritura atómica del bus: la de `ea-plataforma` (temporal oculto en el
//! mismo directorio, `fsync` y renombrado) creando antes el directorio.

use std::fs;
use std::path::Path;

use ea_plataforma::fs::Permisos;
use serde::Serialize;

use crate::error::{ErrorBus, Resultado};

pub fn escribir_atomico(destino: &Path, contenido: &[u8]) -> Resultado<()> {
    let dir = destino.parent().ok_or_else(|| ErrorBus::Argumentos("ruta sin directorio".into()))?;
    fs::create_dir_all(dir)?;
    ea_plataforma::fs::escribir_atomico(destino, contenido, Permisos::Heredados)?;
    Ok(())
}

pub fn escribir_json<T: Serialize>(destino: &Path, valor: &T) -> Resultado<()> {
    escribir_atomico(destino, &serde_json::to_vec_pretty(valor)?)
}
