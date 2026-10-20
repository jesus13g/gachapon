//! El historial de eventos.

use ea_store::{Event, EventFilter, EventOutcome, NuevaNota};

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
    let filter = EventFilter {
        trabajo: f.trabajo.filter(|t| !t.is_empty()),
        actor: f.actor.filter(|a| !a.is_empty()),
        since: f.desde.as_deref().map(parse_fecha).transpose()?,
        until: f.hasta.as_deref().map(parse_fecha).transpose()?,
        exclude_action: (!f.latidos).then(|| "heartbeat".to_string()),
        outcome,
        action_prefix: accion,
        limit: Some(limite(f.limite, 200, 1000)),
    };
    let store = s.store.lock().await;
    let eventos = store.list_events(&filter)?;
    let ids: Vec<i64> = eventos.iter().map(|e| e.id).collect();
    let mut correcciones = store.correcciones_diario(&ids)?;
    Ok(eventos
        .iter()
        .map(|e| EventoVista { corregido_por: correcciones.remove(&e.id).unwrap_or_default(), ..vista(e) })
        .collect())
}

/// Añade una nota del usuario (`usuario_nota`) al diario, que puede corregir
/// otro evento. No toca ningún evento existente: el diario es append-only.
pub async fn anotar(s: &AppState, nota: NuevaNota) -> Resultado<EventoVista> {
    let store = s.store.lock().await;
    Ok(vista(&store.anotar_diario(nota)?))
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
        corregido_por: Vec::new(),
    }
}
