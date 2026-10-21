//! Salud del orquestador derivada del diario (§13.2): el propio orquestador
//! emite `actor='orquestador', action='heartbeat'` y las interfaces solo
//! leen el último. Compartido por `ea status`, la TUI, `ea-mcp` y `ea-web`.

use time::{Duration, OffsetDateTime};

use crate::error::StoreResult;
use crate::models::Event;
use crate::repository::events::EventFilter;
use crate::store::Store;

/// Cuántos eventos recientes del orquestador se miran para hallar el latido.
const VENTANA: i64 = 50;

/// Último heartbeat de una lista de eventos. Ignora los fechados en el
/// futuro: el diario es append-only y un arranque con el reloj mal puesto
/// (esta máquina alterna con Windows) deja filas que, ordenadas por `ts`,
/// taparían a los latidos reales y darían por vivo a un orquestador caído.
pub fn ultimo_latido(eventos: &[Event], ahora: OffsetDateTime) -> Option<OffsetDateTime> {
    const MARGEN: Duration = Duration::seconds(5);
    eventos
        .iter()
        .filter(|e| e.action == "heartbeat" && e.ts <= ahora + MARGEN)
        .map(|e| e.ts)
        .max()
}

/// Estado del orquestador según su último latido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EstadoOrquestador {
    /// `true` si el último latido no es más antiguo que el umbral.
    pub vivo: bool,
    /// `None` si nunca ha latido (no es lo mismo que «caído»).
    pub ultimo_latido: Option<OffsetDateTime>,
}

impl EstadoOrquestador {
    pub fn calcular(eventos: &[Event], ahora: OffsetDateTime, umbral_s: i64) -> Self {
        let ultimo_latido = ultimo_latido(eventos, ahora);
        let vivo = ultimo_latido.is_some_and(|t| (ahora - t).whole_seconds() <= umbral_s);
        Self { vivo, ultimo_latido }
    }
}

impl Store {
    /// Estado del orquestador ahora mismo; `umbral_s` es la antigüedad
    /// máxima tolerada del último latido (el doble del intervalo).
    pub fn estado_orquestador(&self, umbral_s: i64) -> StoreResult<EstadoOrquestador> {
        let eventos = self.list_events(&EventFilter {
            actor: Some("orquestador".into()),
            limit: Some(VENTANA),
            ..Default::default()
        })?;
        Ok(EstadoOrquestador::calcular(&eventos, OffsetDateTime::now_utc(), umbral_s))
    }
}

/// Actores de IA que las interfaces muestran como agentes: los dos roles, con
/// su nombre en disco (el planificador es `claude-code` y el ejecutor
/// `modelo-local`, los ocupe el agente que los ocupe; spec 017).
pub const AGENTES: [&str; 2] = ["claude-code", "modelo-local"];

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
                    .list_events(&EventFilter {
                        actor: Some(nombre.into()),
                        exclude_action: Some("heartbeat".into()),
                        limit: Some(1),
                        ..Default::default()
                    })?
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

    fn latido(ts: OffsetDateTime) -> Event {
        Event {
            id: 0,
            ts,
            actor: "orquestador".into(),
            action: "heartbeat".into(),
            trabajo: None,
            outcome: None,
            duration_ms: None,
            detail: None,
        }
    }

    #[test]
    fn el_ultimo_latido_ignora_los_fechados_en_el_futuro() {
        let ahora = OffsetDateTime::now_utc();
        let real = ahora - Duration::seconds(10);
        let eventos = [
            latido(ahora + Duration::hours(1)), // reloj mal puesto al arrancar
            latido(real),
            latido(real - Duration::seconds(30)),
        ];
        assert_eq!(ultimo_latido(&eventos, ahora), Some(real));
        assert_eq!(ultimo_latido(&eventos[..1], ahora), None);
    }

    #[test]
    fn el_estado_depende_del_umbral() {
        let ahora = OffsetDateTime::now_utc();
        let eventos = [latido(ahora - Duration::seconds(40))];
        assert!(EstadoOrquestador::calcular(&eventos, ahora, 60).vivo);
        let caido = EstadoOrquestador::calcular(&eventos, ahora, 30);
        assert!(!caido.vivo);
        assert!(caido.ultimo_latido.is_some());
        assert_eq!(EstadoOrquestador::calcular(&[], ahora, 60).ultimo_latido, None);
    }
}
