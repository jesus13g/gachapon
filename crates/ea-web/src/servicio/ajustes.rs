//! Los ajustes del ecosistema (`ajustes.toml`, vía `ea-ajustes`). Es un gesto
//! directo del usuario, como los editores de Memoria y Bóveda: no pasa por la
//! capa de acciones ni existe en ea-mcp. Desde aquí solo se tocan las claves
//! marcadas para la web, lo endurecible solo baja y cada cambio queda en el
//! diario.

use std::collections::BTreeMap;

use ea_ajustes::{Canal, ErrorAjustes, cargar_de};
use serde_json::Value;

use super::{ErrorWeb, Resultado};
use crate::AppState;
use crate::vistas::{AjusteAplicado, AjustesCambiados, AjustesVista};

impl From<ErrorAjustes> for ErrorWeb {
    fn from(e: ErrorAjustes) -> Self {
        let m = e.to_string();
        match e {
            ErrorAjustes::ClaveDesconocida(_) | ErrorAjustes::ValorInvalido { .. } | ErrorAjustes::SinCambios => Self::Invalido(m),
            ErrorAjustes::NoEditable { .. } | ErrorAjustes::SoloEndurecer { .. } => Self::Prohibido(m),
            ErrorAjustes::FijadaPorEntorno { .. } | ErrorAjustes::Version | ErrorAjustes::FicheroInvalido(_) => Self::Conflicto(m),
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
    // El caso de uso entero (el fichero, el diario y el autoarranque de
    // `escritorio.al_iniciar`) es E/S bloqueante (ficheros, `reg` en Windows):
    // fuera del ejecutor async, con su propia conexión al diario.
    let diario = s.store.lock().await.duplicar();
    let (fuentes, version) = (s.config.ajustes.clone(), version.to_string());
    let c = tokio::task::spawn_blocking(move || ea_acciones::ajustes::cambiar(&fuentes, Canal::Web, Some(&version), &lote, || diario))
        .await
        .map_err(|e| ErrorWeb::Interno(format!("no se pudo aplicar el cambio de ajustes: {e}")))??;
    // La escritura ya está hecha: si el diario o un efecto fallan, se avisa en el log.
    if let Some(e) = &c.sin_diario {
        tracing::warn!(error = %e, "no se pudo apuntar el cambio de ajustes en el diario");
    }
    for aviso in &c.avisos {
        tracing::warn!("{aviso}");
    }

    Ok(AjustesCambiados {
        aplicados: c
            .aplicados
            .into_iter()
            .map(|a| AjusteAplicado { clave: a.clave, antes: a.antes, despues: a.despues, aplica: a.aplica.nombre(), reinicio: a.aplica.servicios() })
            .collect(),
        ajustes: ver(s),
        avisos: c.avisos,
    })
}
