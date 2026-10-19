//! La memoria de Claude Code: listar, leer, guardar, renombrar, borrar y
//! mostrar en el explorador. El usuario la edita a mano desde la página; cada
//! cambio deja rastro en el diario (el borrado, con el contenido).

use ea_store::{EventOutcome, NewEvent};
use serde_json::json;

use super::{Resultado, ErrorWeb, ACTOR};
use crate::memoria::{self, ErrorMemoria};
use crate::vistas::{Hecho, ListadoMemoria, Renombrado};
use crate::AppState;

impl From<ErrorMemoria> for ErrorWeb {
    fn from(e: ErrorMemoria) -> Self {
        match e {
            ErrorMemoria::NoEncontrado(m) => Self::NoEncontrado(m),
            ErrorMemoria::Invalido(m) => Self::Invalido(m),
            ErrorMemoria::Conflicto(m) => Self::Conflicto(m),
            ErrorMemoria::NoDisponible(m) => Self::NoDisponible(m),
            ErrorMemoria::Interno(m) => Self::Interno(m),
        }
    }
}

fn raiz(s: &AppState) -> Resultado<&std::path::Path> {
    s.config
        .proyectos_claude_dir
        .as_deref()
        .ok_or_else(|| ErrorWeb::NoDisponible("la memoria de Claude Code no está configurada".into()))
}

pub fn listar(s: &AppState) -> Resultado<ListadoMemoria> {
    let raiz = raiz(s)?;
    Ok(ListadoMemoria { raiz: raiz.display().to_string(), proyectos: memoria::listar(raiz) })
}

pub fn leer(s: &AppState, proyecto: &str, fichero: &str) -> Resultado<memoria::Memoria> {
    Ok(memoria::leer(raiz(s)?, proyecto, fichero)?)
}

/// La escritura ya está hecha: si el diario falla, se avisa en el log y se da
/// la operación por buena.
async fn apunta(s: &AppState, accion: &str, detalle: serde_json::Value) {
    let evento = NewEvent {
        actor: ACTOR.into(),
        action: accion.into(),
        trabajo: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(detalle.to_string()),
    };
    if let Err(e) = s.store.lock().await.append_event(evento) {
        tracing::warn!(error = %e, accion, "no se pudo apuntar el cambio de memoria en el diario");
    }
}

pub async fn guardar(
    s: &AppState,
    proyecto: &str,
    fichero: &str,
    contenido: &str,
    version: &str,
) -> Resultado<memoria::Memoria> {
    let m = memoria::guardar(raiz(s)?, proyecto, fichero, contenido, version)?;
    apunta(s, "memoria_editada", json!({ "proyecto": proyecto, "fichero": fichero, "bytes": contenido.len() })).await;
    Ok(m)
}

/// Borrado definitivo (constitución IV.2, excepción 1.1.0): el contenido queda
/// en el diario, que es append-only.
pub async fn eliminar(s: &AppState, proyecto: &str, fichero: &str, version: &str) -> Resultado<Hecho> {
    let contenido = memoria::eliminar(raiz(s)?, proyecto, fichero, version)?;
    let detalle = json!({ "proyecto": proyecto, "fichero": fichero, "bytes": contenido.len(), "contenido": contenido });
    apunta(s, "memoria_eliminada", detalle).await;
    Ok(Hecho { ok: true })
}

pub async fn renombrar(s: &AppState, proyecto: &str, fichero: &str, nuevo: &str, version: &str) -> Resultado<Renombrado> {
    let (m, actualizadas) = memoria::renombrar(raiz(s)?, proyecto, fichero, nuevo, version)?;
    apunta(s, "memoria_renombrada", json!({ "proyecto": proyecto, "de": fichero, "a": nuevo, "actualizadas": actualizadas })).await;
    Ok(Renombrado { memoria: m, actualizadas })
}

/// Abre el gestor de ficheros del escritorio con la memoria seleccionada
/// (`FileManager1.ShowItems` por D-Bus) o, si no hay quien lo atienda, su
/// carpeta con `xdg-open`. La ruta va como argumento, nunca por un shell.
pub async fn mostrar(s: &AppState, proyecto: &str, fichero: &str) -> Resultado<Hecho> {
    let m = memoria::leer(raiz(s)?, proyecto, fichero)?;
    let ruta = std::path::PathBuf::from(&m.ruta);
    let uri = uri_fichero(&m.ruta);
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
    let carpeta = ruta.parent().unwrap_or(&ruta);
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
