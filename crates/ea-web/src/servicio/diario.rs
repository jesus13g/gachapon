//! El historial de eventos.

use ea_store::{Event, EventFilter, FiltroDiario, NuevaNota};

use super::{ts, Resultado};
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
    /// Incluir los heartbeats que el orquestador anotaba antes en el diario (por
    /// defecto se omiten; ahora el latido va a `orquestador.json` del bus).
    pub latidos: bool,
    /// Cursor «cargar más»: el id del último evento ya cargado; se devuelven
    /// los anteriores a él con los mismos filtros.
    pub antes_de: Option<String>,
    pub limite: Option<i64>,
}

pub async fn listar(s: &AppState, f: Filtro) -> Resultado<Vec<EventoVista>> {
    // Un campo vacío del formulario es que no se filtra por él (las fechas
    // vacías, en cambio, no valen).
    let lleno = |v: Option<String>| v.filter(|t| !t.is_empty());
    let filtro = FiltroDiario {
        trabajo: lleno(f.trabajo),
        actor: lleno(f.actor),
        desde: f.desde,
        hasta: f.hasta,
        resultado: lleno(f.resultado),
        accion: lleno(f.accion),
        antes_de: lleno(f.antes_de),
        limite: Some(f.limite.unwrap_or(200)),
    };
    let filter = EventFilter { exclude_action: (!f.latidos).then(|| "heartbeat".to_string()), ..filtro.a_filtro(Some(1000))? };
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
