//! Mostrar un fichero en el explorador de archivos del escritorio (lo usan
//! la memoria de Claude Code y la bóveda).

use std::path::Path;

use super::{ErrorWeb, Resultado};
use crate::vistas::Hecho;

/// Abre el gestor de ficheros del escritorio con el fichero seleccionado
/// (`FileManager1.ShowItems` por D-Bus) o, si no hay quien lo atienda, su
/// carpeta con `xdg-open`. La ruta va como argumento, nunca por un shell.
pub async fn mostrar(ruta: &Path) -> Resultado<Hecho> {
    let uri = uri_fichero(&ruta.display().to_string());
    let dbus = tokio::process::Command::new("gdbus")
        .args(["call", "--session", "--timeout", "5", "--dest", "org.freedesktop.FileManager1", "--object-path",
            "/org/freedesktop/FileManager1", "--method", "org.freedesktop.FileManager1.ShowItems"])
        .arg(format!("['{uri}']"))
        .arg("")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    if dbus.is_ok_and(|st| st.success()) {
        return Ok(Hecho { ok: true });
    }
    let carpeta = ruta.parent().unwrap_or(ruta);
    tokio::process::Command::new("xdg-open")
        .arg(carpeta)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| ErrorWeb::NoDisponible(format!("no hay explorador de archivos que abrir: {e}")))?;
    Ok(Hecho { ok: true })
}

/// `file://` con todo lo que no es seguro en un URI (ni en la cadena GVariant
/// entre comillas simples que lo lleva) codificado en `%XX`.
fn uri_fichero(ruta: &str) -> String {
    let mut s = String::from("file://");
    for b in ruta.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(char::from(b));
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}
