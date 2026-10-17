//! La memoria de Claude Code: listar, leer y guardar. El usuario la edita a
//! mano desde la página; cada guardado deja rastro en el diario.

use ea_store::{EventOutcome, NewEvent};
use serde_json::json;

use super::{Resultado, ErrorWeb, ACTOR};
use crate::memoria::{self, ErrorMemoria};
use crate::vistas::ListadoMemoria;
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

pub async fn guardar(
    s: &AppState,
    proyecto: &str,
    fichero: &str,
    contenido: &str,
    version: &str,
) -> Resultado<memoria::Memoria> {
    let m = memoria::guardar(raiz(s)?, proyecto, fichero, contenido, version)?;
    // La escritura ya está hecha: si el diario falla, se avisa en el log y se
    // da el guardado por bueno.
    let evento = NewEvent {
        actor: ACTOR.into(),
        action: "memoria_editada".into(),
        trabajo: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(json!({ "proyecto": proyecto, "fichero": fichero, "bytes": contenido.len() }).to_string()),
    };
    if let Err(e) = s.store.lock().await.append_event(evento) {
        tracing::warn!(error = %e, "no se pudo apuntar la edición de memoria en el diario");
    }
    Ok(m)
}
