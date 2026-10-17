//! Vista general, portada y sesiones de Claude Code.

use ea_bus::Bus;
use ea_store::{EstadoPropuesta, PropuestaFilter};
use time::OffsetDateTime;

use super::{propuestas::vista as propuesta_vista, ts, Resultado};
use crate::vistas::{AgenteVista, EstadoGeneral, Inicio, Orquestador, ServicioVista, Sesiones, TrabajosPorEstado, UltimoEvento};
use crate::AppState;

pub async fn general(s: &AppState) -> Resultado<EstadoGeneral> {
    let servicios = crate::estado::servicios(s.config.systemctl).await;

    let store = s.store.lock().await;
    let orquestador = store.estado_orquestador(s.config.umbral_heartbeat_s)?;

    let agentes = store
        .resumen_agentes()?
        .into_iter()
        .map(|a| AgenteVista {
            nombre: a.nombre,
            ultimo: a.ultimo_evento.map(|e| UltimoEvento { ts: ts(e.ts), accion: e.action }),
        })
        .collect();

    let propuestas_pendientes = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: None })?
        .len();

    Ok(EstadoGeneral {
        ahora: ts(OffsetDateTime::now_utc()),
        servicios: servicios
            .into_iter()
            .map(|sv| ServicioVista { nombre: sv.nombre, activo: sv.activo, activo_s: sv.activo_s })
            .collect(),
        orquestador: Orquestador { vivo: orquestador.vivo, ultimo_latido: orquestador.ultimo_latido.map(ts) },
        agentes,
        trabajos: trabajos_por_estado(s),
        propuestas_pendientes,
        db: ea_store::resolve_db_path().ok().map(|p| p.display().to_string()),
    })
}

/// Trabajos del bus por estado; todo a cero si el bus no se puede leer.
fn trabajos_por_estado(s: &AppState) -> TrabajosPorEstado {
    let mut t = TrabajosPorEstado::default();
    let Ok(bus) = Bus::abrir_en(&s.config.estado_dir) else { return t };
    for id in bus.listar_trabajos().unwrap_or_default() {
        let Ok(e) = bus.leer_estado(&id) else { continue };
        match e.estado.as_str() {
            "en_cola" => t.en_cola += 1,
            "ejecutando" => t.ejecutando += 1,
            "en_revision" => t.en_revision += 1,
            "en_espera" => t.en_espera += 1,
            "cerrado" => t.cerrado += 1,
            "cancelado" => t.cancelado += 1,
            _ => {}
        }
    }
    t
}

/// Las propuestas pendientes. Los trabajos del orquestador van por
/// `trabajos`, `colas` y `semaforos`.
pub async fn inicio(s: &AppState) -> Resultado<Inicio> {
    let store = s.store.lock().await;
    let propuestas = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: Some(20) })?
        .iter()
        .map(propuesta_vista)
        .collect();
    Ok(Inicio { propuestas })
}

pub fn sesiones(s: &AppState) -> Sesiones {
    let subagentes = s.config.subagentes_dir.as_deref();
    Sesiones {
        sesiones: s.config.sesiones_dir.as_deref().map(|d| crate::sesiones::vivas(d, subagentes)).unwrap_or_default(),
    }
}
