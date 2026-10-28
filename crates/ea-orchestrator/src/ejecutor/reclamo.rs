//! Primer paso de un intento: reclamar la siguiente entrada de las colas del
//! ejecutor y dejar lista su orden. Una entrada que no lleva a nada que
//! ejecutar (orden ilegible, trabajo que ya no está en cola) se descarta
//! aquí, sin crear worktree ni tocar el estado.

use ea_bus::{Artefactos, Bus, EstadoTrabajo, Orden};

use super::Intento;
use crate::config::ConfigOrquestador;

/// Un trabajo reclamado: su orden, con los topes ya acotados, y sus artefactos.
pub(super) struct Reclamo {
    pub(super) id: String,
    pub(super) orden: Orden,
    pub(super) artefactos: Artefactos,
}

/// La siguiente entrada para el ejecutor de `config`: primero la de su cola
/// y luego la de rol que pase `admite` (ver [`Bus::tomar_para_con`]). `Err`
/// con el intento que acaba aquí: [`Intento::Nada`] si no había nada (o la
/// cola falló) y [`Intento::Hecho`] si la entrada se descartó.
pub(super) fn reclamar(bus: &Bus, config: &ConfigOrquestador, admite: &dyn Fn(&Orden) -> bool) -> Result<Reclamo, Intento> {
    let id = match bus.tomar_para_con(&config.ejecutor.id, admite) {
        Ok(Some(id)) => id,
        Ok(None) => return Err(Intento::Nada),
        Err(e) => {
            tracing::warn!(error = %e, "no se pudo leer la cola del ejecutor");
            return Err(Intento::Nada);
        }
    };
    let (mut orden, artefactos) = match bus.leer_orden(&id).and_then(|o| Ok((o, bus.artefactos(&id)?))) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(%id, error = %e, "orden ilegible; se descarta la entrada");
            return Err(Intento::Hecho(id));
        }
    };
    // Una entrada rancia (el trabajo ya no está en cola: se canceló, se cerró
    // o lo tomó otro) se descarta sin crear worktree ni tocar el estado.
    match bus.leer_estado(&id) {
        Ok(e) if e.estado == EstadoTrabajo::EnCola => {}
        otro => {
            tracing::warn!(%id, estado = ?otro.map(|e| e.estado), "entrada rancia: el trabajo no está en cola; se descarta");
            return Err(Intento::Hecho(id));
        }
    }
    // Ninguna orden pasa del techo de los ajustes, la escriba quien la escriba.
    let acotados = orden.topes.acotar(&config.techo_topes);
    if acotados != orden.topes {
        tracing::warn!(%id, pedidos = ?orden.topes, techo = ?config.techo_topes, "topes acotados al techo de los ajustes");
        orden.topes = acotados;
    }
    Ok(Reclamo { id, orden, artefactos })
}

#[cfg(test)]
mod tests {
    use crate::ejecutor::pruebas::*;
    use crate::ejecutor::*;
    use ea_bus::{EstadoTrabajo, Rol, Topes};

    #[test]
    fn los_topes_de_la_orden_se_recortan_al_techo() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let topes = Topes { turnos: 500, tiempo_s: 9000, llamadas_herramienta: 20, iteraciones: 5 };
        bus.crear_trabajo(Orden { topes, ..orden(wt.path(), "false") }).unwrap();
        let cfg = ConfigOrquestador {
            techo_topes: Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 200, iteraciones: 1 },
            ..config()
        };
        let ag = doble("true", 0);
        ejecutar_uno(&bus, &cfg, &ag);
        let vistos = ag.topes.lock().unwrap();
        // techo de iteraciones 1: una sola llamada aunque el criterio falle
        assert_eq!(vistos.len(), 1);
        assert_eq!(vistos[0], Topes { turnos: 10, tiempo_s: 120, llamadas_herramienta: 20, iteraciones: 1 });
    }

    /// Una orden ilegible se descarta en el reclamo: ni semáforo, ni
    /// presencia, ni cambio de estado, y la entrada no vuelve.
    #[test]
    fn una_orden_ilegible_se_descarta_sin_tocar_nada() {
        let wt = repo();
        let e = tempfile::tempdir().unwrap();
        let bus = Bus::abrir_en(e.path()).unwrap();
        let id = bus.crear_trabajo(orden(wt.path(), "true")).unwrap();
        std::fs::write(e.path().join("trabajos").join(&id).join("orden.json"), "{").unwrap();
        let ag = doble("true", 0);
        let (al_cambiar, vistas) = presencia(bus.raiz(), wt.path());
        let r = ejecutar_intento(&bus, &config(), &ag, ESPERA_CANDADO, &al_cambiar, &|_| true, &|| false);
        assert_eq!(r, Intento::Hecho(id.clone()));
        assert!(ag.visto.lock().unwrap().is_empty() && vistas.lock().unwrap().is_empty());
        assert_eq!(bus.leer_estado(&id).unwrap().estado, EstadoTrabajo::EnCola);
        assert!(bus.listar_cola(Rol::Ejecutor).unwrap().is_empty());
        assert!(ea_bus::listar_semaforos_en(bus.raiz()).is_empty());
    }
}
