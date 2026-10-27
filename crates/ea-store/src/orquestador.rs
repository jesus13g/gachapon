//! Salud del orquestador derivada del diario (§13.2): el propio orquestador
//! emite `actor='orquestador', action='heartbeat'` y las interfaces solo
//! leen el último. Compartido por `ea status`, la TUI, `ea-mcp` y `ea-web`.

use time::{Duration, OffsetDateTime};

use crate::error::StoreResult;
use crate::models::Event;
use crate::repository::events::EventFilter;
use crate::store::Store;

/// Cuánto puede adelantarse un latido al reloj de quien lo lee.
const MARGEN: Duration = Duration::seconds(5);

/// Último heartbeat de una lista de eventos. Ignora los fechados en el
/// futuro: el diario es append-only y un arranque con el reloj mal puesto
/// (esta máquina alterna con Windows) deja filas que, ordenadas por `ts`,
/// taparían a los latidos reales y darían por vivo a un orquestador caído.
pub fn ultimo_latido(eventos: &[Event], ahora: OffsetDateTime) -> Option<OffsetDateTime> {
    eventos
        .iter()
        .filter(|e| e.action == "heartbeat" && e.ts <= ahora + MARGEN)
        .map(|e| e.ts)
        .max()
}

/// Aislamiento del ejecutor que el orquestador declara en su latido
/// (`detail` JSON; spec 020, ampliación «sandbox visible»).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SandboxInfo {
    /// `Linux`, `macOS`, `Windows` u «otro sistema».
    pub sistema: String,
    /// `bwrap`, `seatbelt`, `ninguno` o `no disponible`.
    pub sandbox: String,
    pub disponible: bool,
    /// Por qué no puede arrancar (solo si `disponible` es falso).
    #[serde(default)]
    pub motivo: Option<String>,
}

/// El sandbox declarado por el último latido (el más reciente por `ts`, sin
/// contar los del futuro). `None` si no hay latido o es de antes de este dato.
pub fn ultimo_sandbox(eventos: &[Event], ahora: OffsetDateTime) -> Option<SandboxInfo> {
    eventos
        .iter()
        .filter(|e| e.action == "heartbeat" && e.ts <= ahora + MARGEN)
        .max_by_key(|e| e.ts)
        .and_then(|e| e.detail.as_deref())
        .and_then(|d| serde_json::from_str(d).ok())
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

    /// «vivo», «sin señal reciente» o «sin señal (ningún heartbeat
    /// registrado todavía)»: lo que dicen `ea status`, la TUI y `ea_status`.
    /// Nunca «caído»: sin latido no se sabe más.
    pub fn descripcion(&self) -> &'static str {
        match (self.vivo, self.ultimo_latido) {
            (true, _) => "vivo",
            (false, Some(_)) => "sin señal reciente",
            (false, None) => "sin señal (ningún heartbeat registrado todavía)",
        }
    }
}

impl Store {
    /// El último latido del orquestador, filtrado en SQL: la acción
    /// `heartbeat`, sin los fechados más allá de `ahora` + [`MARGEN`]. Los
    /// demás eventos del orquestador (los del bus) no lo tapan por muchos
    /// que sean.
    fn latido(&self, ahora: OffsetDateTime) -> StoreResult<Vec<Event>> {
        self.list_events(&EventFilter {
            actor: Some("orquestador".into()),
            action_prefix: Some("heartbeat".into()),
            until: Some(ahora + MARGEN),
            limit: Some(1),
            ..Default::default()
        })
    }

    /// Estado del orquestador ahora mismo; `umbral_s` es la antigüedad
    /// máxima tolerada del último latido (el doble del intervalo).
    pub fn estado_orquestador(&self, umbral_s: i64) -> StoreResult<EstadoOrquestador> {
        let ahora = OffsetDateTime::now_utc();
        Ok(EstadoOrquestador::calcular(&self.latido(ahora)?, ahora, umbral_s))
    }

    /// Sandbox que declara el último latido del orquestador.
    pub fn sandbox_orquestador(&self) -> StoreResult<Option<SandboxInfo>> {
        let ahora = OffsetDateTime::now_utc();
        Ok(ultimo_sandbox(&self.latido(ahora)?, ahora))
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

    /// B-13: muchos eventos del orquestador tras el último latido (los del
    /// bus de un trabajo) no lo tapan: sigue vivo y con su sandbox.
    #[test]
    fn un_latido_seguido_de_muchos_eventos_del_orquestador_sigue_vivo() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open_at(&d.path().join("t.db")).unwrap();
        let ev = |action: &str, detail: Option<&str>| crate::NewEvent {
            actor: "orquestador".into(),
            action: action.into(),
            trabajo: None,
            outcome: Some(crate::EventOutcome::Ok),
            duration_ms: None,
            detail: detail.map(Into::into),
        };
        s.append_event(ev("heartbeat", Some(r#"{"sistema":"Linux","sandbox":"bwrap","disponible":true}"#))).unwrap();
        for _ in 0..60 {
            s.append_event(ev("bus_tomar", None)).unwrap();
        }
        let e = s.estado_orquestador(60).unwrap();
        assert!(e.vivo && e.ultimo_latido.is_some(), "{e:?}");
        assert_eq!(s.sandbox_orquestador().unwrap().map(|i| i.sandbox), Some("bwrap".into()));
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

    #[test]
    fn la_descripcion_distingue_vivo_sin_senal_reciente_y_sin_ningun_latido() {
        let ahora = OffsetDateTime::now_utc();
        let eventos = [latido(ahora - Duration::seconds(40))];
        assert_eq!(EstadoOrquestador::calcular(&eventos, ahora, 60).descripcion(), "vivo");
        assert_eq!(EstadoOrquestador::calcular(&eventos, ahora, 30).descripcion(), "sin señal reciente");
        assert_eq!(EstadoOrquestador::calcular(&[], ahora, 60).descripcion(), "sin señal (ningún heartbeat registrado todavía)");
    }

    #[test]
    fn el_sandbox_sale_del_ultimo_latido_que_lo_trae() {
        let ahora = OffsetDateTime::now_utc();
        let mut viejo = latido(ahora - Duration::seconds(60));
        viejo.detail = Some(r#"{"sistema":"Linux","sandbox":"bwrap","disponible":true}"#.into());
        let mut nuevo = latido(ahora - Duration::seconds(10));
        nuevo.detail = Some(
            r#"{"sistema":"Windows","sandbox":"no disponible","disponible":false,"motivo":"sin sandbox"}"#.into(),
        );
        let info = ultimo_sandbox(&[viejo.clone(), nuevo], ahora).unwrap();
        assert!(!info.disponible);
        assert_eq!(info.motivo.as_deref(), Some("sin sandbox"));
        // Un latido anterior a este dato (sin detalle) no inventa nada.
        assert_eq!(ultimo_sandbox(&[latido(ahora)], ahora), None);
        assert!(ultimo_sandbox(&[viejo], ahora).unwrap().disponible);
    }
}
