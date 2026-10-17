//! Reinicio de los servicios residentes.

use ea_acciones::Canal;
use serde_json::json;

use super::{sistema, ErrorWeb, Resultado, ACTOR};
use crate::vistas::Hecho;
use crate::AppState;

pub async fn reiniciar(s: &AppState, nombre: &str) -> Resultado<Hecho> {
    if !crate::estado::SERVICIOS.contains(&nombre) {
        return Err(ErrorWeb::NoEncontrado("servicio desconocido".into()));
    }
    if !s.config.systemctl {
        return Err(ErrorWeb::NoDisponible("systemctl desactivado".into()));
    }

    let sis = sistema(s);
    let mut store = s.acciones.lock().await;
    ea_acciones::ejecutar_directo(
        &mut store, &sis, ACTOR, Canal::Web, "ea_reiniciar_servicio", &json!({ "servicio": nombre }),
    )
    .await?;
    Ok(Hecho { ok: true })
}
