//! `GET /api/ecosistema`: lee las fuentes reales (sesiones de Claude Code,
//! marcas del hook de subagentes, presencias, semáforos y trabajos del bus,
//! últimos eventos del diario) y deja que `crate::ecosistema` las componga.

use ea_ajustes::Rol;
use ea_store::EventFilter;

use super::bus_lectura;
use crate::AppState;
use crate::ecosistema::{self, Ecosistema, EventoBus, Roles, SemaforoBus, TrabajoBus};

/// Cuántos eventos recientes del diario viajan como señales.
const SENALES: i64 = 12;

pub async fn vista(s: &AppState) -> Ecosistema {
    let ajustes = ea_ajustes::cargar_de(&s.config.ajustes).ajustes;
    let roles = Roles { planificador: ajustes.agente_de(Rol::Planificador).0.to_string(), ejecutor: ajustes.agente_de(Rol::Ejecutor).0.to_string() };
    let ahora_ms = time::OffsetDateTime::now_utc().unix_timestamp_nanos() as u64 / 1_000_000;

    // Los semáforos de antes de la 017 llevan el rol en disco: su agente es el del rol.
    let semaforos: Vec<SemaforoBus> = ea_bus::listar_semaforos_en(&s.config.estado_dir)
        .into_iter()
        .map(|(_, o)| SemaforoBus {
            agente: match Rol::desde(&o.agente) {
                Some(r) if ea_ajustes::ficha(&o.agente).is_none() => ajustes.agente_de(r).0.to_string(),
                _ => o.agente,
            },
            trabajo: o.trabajo,
        })
        .collect();

    let mut agentes = Vec::new();
    // Detector de Claude Code.
    if let Some(dir) = s.config.sesiones_dir.as_deref() {
        let sesiones = crate::sesiones::vivas(dir, s.config.subagentes_dir.as_deref());
        let proyectos = s.config.proyectos_claude_dir.as_deref();
        let modelo = |x: &crate::sesiones::Sesion| proyectos.and_then(|p| crate::sesiones::modelo_sesion(p, &x.cwd, &x.sesion));
        let rol = ajustes.rol_de("claude-code").map(Rol::nombre);
        agentes.extend(ecosistema::claude::detectar(sesiones, modelo, rol, ahora_ms));
    }
    // Detector del bus.
    let bus = bus_lectura(s).ok();
    if let Some(b) = &bus {
        agentes.extend(ecosistema::bus::detectar(&b.presencias(), &semaforos, &ajustes));
    }

    let trabajos: Vec<TrabajoBus> = bus.as_ref().map(trabajos_activos).unwrap_or_default();

    let eventos = {
        let store = s.store.lock().await;
        let filtro = EventFilter { exclude_action: Some("heartbeat".into()), limit: Some(SENALES), ..Default::default() };
        store.list_events(&filtro).unwrap_or_default()
    };
    let eventos: Vec<EventoBus> = eventos
        .into_iter()
        // Los cambios de estado del bus ya los cuentan las cápsulas.
        .filter(|e| !e.action.starts_with("bus_"))
        .map(|e| EventoBus { id: e.id, error: e.outcome.is_some_and(|o| o.as_str() == "error"), actor: e.actor, accion: e.action, trabajo: e.trabajo })
        .collect();

    ecosistema::componer(agentes, &trabajos, &semaforos, &eventos, &roles)
}

/// Los trabajos que no están cerrados ni cancelados, con lo que hace falta
/// para ubicarlos.
fn trabajos_activos(bus: &ea_bus::Bus) -> Vec<TrabajoBus> {
    bus.listar_trabajos()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|id| {
            let estado = bus.leer_estado(&id).ok()?;
            let estado_txt = estado.estado.as_str();
            if matches!(estado_txt, "cerrado" | "cancelado") {
                return None;
            }
            let orden = bus.leer_orden(&id).ok()?;
            Some(TrabajoBus {
                id,
                estado: estado_txt.into(),
                titulo: orden.titulo,
                worktree: orden.worktree,
                asignado_a: orden.asignado_a,
                revisor: orden.revisor,
                ejecutado_por: estado.ejecutado_por.map(|e| e.agente),
                del_usuario: orden.rutina.is_some() || orden.creada_por == "usuario",
            })
        })
        .collect()
}
