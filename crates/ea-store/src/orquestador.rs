//! Lo que el diario dice de los roles: qué hizo cada uno por última vez.
//! El latido del orquestador ya no está aquí: va a un fichero del bus
//! (`ea_bus::LatidoOrquestador`, U-07); los `heartbeat` de antes siguen en el
//! diario y se excluyen de estos resúmenes.

use ea_ajustes::Rol;

use crate::error::StoreResult;
use crate::models::Event;
use crate::repository::events::EventFilter;
use crate::store::Store;

/// Actores de IA que las interfaces muestran como agentes: los dos roles, con
/// su nombre en disco (el planificador es `claude-code` y el ejecutor
/// `modelo-local`, los ocupe el agente que los ocupe; spec 017).
pub const AGENTES: [&str; 2] = [Rol::Planificador.as_str(), Rol::Ejecutor.as_str()];

/// Qué hizo por última vez un rol según el diario, nunca preguntándole a él.
#[derive(Clone, Debug)]
pub struct ResumenAgente {
    /// El rol, con su nombre en disco.
    pub nombre: &'static str,
    pub ultimo_evento: Option<Event>,
    /// El agente del catálogo que hizo ese último evento, si el diario lo
    /// dice (`detalle.agente`, que anota ea-mcp desde la spec 017).
    pub agente: Option<String>,
}

/// El `agente` del detalle de un evento.
fn agente_de(e: &Event) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(e.detail.as_deref()?).ok()?;
    v.get("agente")?.as_str().map(str::to_string)
}

impl Store {
    pub fn resumen_agentes(&self) -> StoreResult<Vec<ResumenAgente>> {
        AGENTES
            .iter()
            .map(|&nombre| {
                let ultimo_evento = self
                    .list_events(&EventFilter { actor: Some(nombre.into()), exclude_action: Some("heartbeat".into()), limit: Some(1), ..Default::default() })?
                    .into_iter()
                    .next();
                let agente = ultimo_evento.as_ref().and_then(agente_de);
                Ok(ResumenAgente { nombre, ultimo_evento, agente })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_resumen_de_cada_rol_dice_que_agente_hizo_lo_ultimo() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_at(&d.path().join("t.db")).unwrap();
        let ev = |actor: &str, detalle: &str| crate::NewEvent {
            actor: actor.into(),
            action: "mcp_status".into(),
            trabajo: None,
            outcome: Some(crate::EventOutcome::Ok),
            duration_ms: None,
            detail: Some(detalle.into()),
        };
        s.append_event(ev("claude-code", r#"{"args":{}}"#)).unwrap();
        s.append_event(ev("claude-code", r#"{"args":{},"agente":"codex"}"#)).unwrap();
        let r = s.resumen_agentes().unwrap();
        assert_eq!(r.iter().map(|a| a.nombre).collect::<Vec<_>>(), AGENTES);
        assert_eq!(r[0].agente.as_deref(), Some("codex"));
        assert!(r[1].ultimo_evento.is_none() && r[1].agente.is_none());
    }
}
