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
        outcome: Some(EventOutcome::Ok),
        detail: Some(detalle.to_string()),
        ..Default::default()
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

/// Crea una memoria nueva (spec 014, H5): fichero con cabecera y línea en
/// `MEMORY.md`. Nunca pisa una existente.
pub async fn crear(s: &AppState, proyecto: &str, n: &memoria::NuevaMemoria<'_>) -> Resultado<memoria::Memoria> {
    let m = memoria::crear(raiz(s)?, proyecto, n)?;
    let detalle = json!({ "proyecto": proyecto, "fichero": m.nombre, "tipo": n.tipo, "bytes": m.contenido.len() });
    apunta(s, "memoria_creada", detalle).await;
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

/// Abre el gestor de ficheros del escritorio con la memoria seleccionada.
pub async fn mostrar(s: &AppState, proyecto: &str, fichero: &str) -> Resultado<Hecho> {
    let m = memoria::leer(raiz(s)?, proyecto, fichero)?;
    super::explorador::mostrar(std::path::Path::new(&m.ruta)).await
}
