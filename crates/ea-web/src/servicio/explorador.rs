//! Mostrar un fichero en el explorador de archivos del escritorio (lo usan
//! la memoria de Claude Code y la bóveda).

use std::path::Path;

use super::{ErrorWeb, Resultado};
use crate::vistas::Hecho;

/// Abre el explorador de archivos del escritorio con el fichero seleccionado
/// (`ea_plataforma::escritorio::revelar`: D-Bus y `xdg-open` en Linux, `open
/// -R` en macOS, `explorer /select,` en Windows). La ruta va como argumento,
/// nunca por un shell.
pub async fn mostrar(ruta: &Path) -> Resultado<Hecho> {
    let ruta = ruta.to_path_buf();
    // `revelar` puede esperar a D-Bus unos segundos: fuera del ejecutor async.
    tokio::task::spawn_blocking(move || ea_plataforma::escritorio::revelar(&ruta))
        .await
        .map_err(|e| ErrorWeb::Interno(format!("la tarea del explorador falló: {e}")))?
        .map_err(|e| ErrorWeb::NoDisponible(format!("no hay explorador de archivos que abrir: {e}")))?;
    Ok(Hecho { ok: true })
}
