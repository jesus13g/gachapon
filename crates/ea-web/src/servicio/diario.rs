//! El historial de eventos.

use ea_store::{Event, EventFilter};

use super::{limite, parse_fecha, ts, Resultado};
use crate::vistas::EventoVista;
use crate::AppState;

#[derive(Default)]
pub struct Filtro {
    pub trabajo: Option<String>,
    pub actor: Option<String>,
    pub desde: Option<String>,
    pub hasta: Option<String>,
    /// Incluir los heartbeats del orquestador (por defecto se omiten).
    pub latidos: bool,
    pub limite: Option<i64>,
}

pub async fn listar(s: &AppState, f: Filtro) -> Resultado<Vec<EventoVista>> {
    let filter = EventFilter {
        trabajo: f.trabajo.filter(|t| !t.is_empty()),
        actor: f.actor.filter(|a| !a.is_empty()),
        since: f.desde.as_deref().map(parse_fecha).transpose()?,
        until: f.hasta.as_deref().map(parse_fecha).transpose()?,
        exclude_action: (!f.latidos).then(|| "heartbeat".to_string()),
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
