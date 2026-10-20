//! El historial de eventos.

use ea_store::{Event, EventFilter, EventOutcome};

use super::{limite, parse_fecha, ts, ErrorWeb, Resultado};
use crate::vistas::EventoVista;
use crate::AppState;

#[derive(Default)]
pub struct Filtro {
    pub trabajo: Option<String>,
    pub actor: Option<String>,
    pub desde: Option<String>,
    pub hasta: Option<String>,
    /// `ok` o `error`.
    pub resultado: Option<String>,
    /// Prefijo de la acción, la familia de eventos (`bus_`, `mcp_`…).
    pub accion: Option<String>,
    /// Incluir los heartbeats del orquestador (por defecto se omiten).
    pub latidos: bool,
    /// Cursor «cargar más»: el id del último evento ya cargado; se devuelven
    /// los anteriores a él con los mismos filtros.
    pub antes_de: Option<String>,
    pub limite: Option<i64>,
}

pub async fn listar(s: &AppState, f: Filtro) -> Resultado<Vec<EventoVista>> {
    let outcome = f
        .resultado
        .as_deref()
        .filter(|r| !r.is_empty())
        .map(str::parse::<EventOutcome>)
        .transpose()?;
    let accion = f.accion.filter(|a| !a.is_empty());
    if let Some(a) = &accion
        && !a.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return Err(ErrorWeb::Invalido(format!("acción no válida: «{a}» (solo [a-z_])")));
    }
    let before_id = f
        .antes_de
        .as_deref()
        .filter(|a| !a.is_empty())
        .map(|a| {
            a.parse::<i64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| ErrorWeb::Invalido(format!("antes_de no válido: «{a}» (un id de evento)")))
        })
        .transpose()?;
    let filter = EventFilter {
        trabajo: f.trabajo.filter(|t| !t.is_empty()),
        actor: f.actor.filter(|a| !a.is_empty()),
        since: f.desde.as_deref().map(parse_fecha).transpose()?,
        until: f.hasta.as_deref().map(parse_fecha).transpose()?,
        exclude_action: (!f.latidos).then(|| "heartbeat".to_string()),
        outcome,
        action_prefix: accion,
        before_id,
        limit: Some(limite(f.limite, 200, 1000)),
    };
    let store = s.store.lock().await;
    Ok(store.list_events(&filter)?.iter().map(vista).collect())
}

fn vista(e: &Event) -> EventoVista {
    EventoVista {
        id: e.id,
        ts: ts(e.ts),
        actor: e.actor.clone(),
        accion: e.action.clone(),
        trabajo: e.trabajo.clone(),
        resultado: e.outcome.map(|o| o.as_str()),
        duracion_ms: e.duration_ms,
        detalle: e.detail.clone(),
    }
}
