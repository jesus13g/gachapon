//! Los ajustes del ecosistema (`ajustes.toml`, vía `ea-ajustes`). Es un gesto
//! directo del usuario, como los editores de Memoria y Bóveda: no pasa por la
//! capa de acciones ni existe en ea-mcp. Desde aquí solo se tocan las claves
//! marcadas para la web, lo endurecible solo baja y cada cambio queda en el
//! diario.

use std::collections::BTreeMap;

use ea_ajustes::{cargar_de, establecer, Canal, ErrorAjustes};
use ea_store::{EventOutcome, NewEvent};
use serde_json::{json, Value};

use super::{ErrorWeb, Resultado, ACTOR};
use crate::vistas::{AjusteAplicado, AjustesCambiados, AjustesVista};
use crate::AppState;

impl From<ErrorAjustes> for ErrorWeb {
    fn from(e: ErrorAjustes) -> Self {
        let m = e.to_string();
        match e {
            ErrorAjustes::ClaveDesconocida(_) | ErrorAjustes::ValorInvalido { .. } | ErrorAjustes::SinCambios => {
                Self::Invalido(m)
            }
            ErrorAjustes::NoEditable { .. } | ErrorAjustes::SoloEndurecer { .. } => Self::Prohibido(m),
            ErrorAjustes::FijadaPorEntorno { .. } | ErrorAjustes::Version | ErrorAjustes::FicheroInvalido(_) => {
                Self::Conflicto(m)
            }
            ErrorAjustes::Io(_) => Self::Interno(m),
        }
    }
}

/// Se relee en cada petición: lo que cambie la terminal se ve al momento.
pub fn ver(s: &AppState) -> AjustesVista {
    cargar_de(&s.config.ajustes).resumen(&s.config.ajustes)
}

pub async fn cambiar(s: &AppState, version: &str, cambios: BTreeMap<String, Option<Value>>) -> Resultado<AjustesCambiados> {
    let _turno = s.ajustes_escritura.lock().await;
    let lote: Vec<(String, Option<Value>)> = cambios.into_iter().collect();
    let aplicados = establecer(&s.config.ajustes, Canal::Web, Some(version), &lote)?;

    // El efecto de `escritorio.al_iniciar` (autoarranque); si falla, el ajuste
    // ya está guardado y queda en el log.
    for aviso in ea_ajustes::aplicar_efectos(&aplicados) {
        tracing::warn!("{aviso}");
    }

    let detalle = json!({
        "canal": "web",
        "fichero": s.config.ajustes.fichero().display().to_string(),
        "cambios": aplicados.iter().map(|a| json!({ "clave": a.clave, "antes": a.antes, "despues": a.despues })).collect::<Vec<_>>(),
    });
    let evento = NewEvent {
        actor: ACTOR.into(),
        action: "ajustes_cambiados".into(),
        trabajo: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(detalle.to_string()),
    };
    // La escritura ya está hecha: si el diario falla, se avisa en el log.
    if let Err(e) = s.store.lock().await.append_event(evento) {
        tracing::warn!(error = %e, "no se pudo apuntar el cambio de ajustes en el diario");
    }

    Ok(AjustesCambiados {
        aplicados: aplicados
            .into_iter()
            .map(|a| AjusteAplicado {
                clave: a.clave,
                antes: a.antes,
                despues: a.despues,
                aplica: a.aplica.nombre(),
                reinicio: a.aplica.servicios(),
            })
            .collect(),
        ajustes: ver(s),
    })
}
