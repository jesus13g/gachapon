//! `ea_reiniciar_servicio`: reiniciar un servicio de la lista cerrada.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{campo, Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, EstadoServicio, Sistema, SERVICIOS_REINICIABLES};

pub(super) const CAMPOS: &[Campo] = &[Campo { opciones: &SERVICIOS_REINICIABLES, ..campo("servicio", "servicio") }];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    servicio: String,
}

pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    if SERVICIOS_REINICIABLES.contains(&a.servicio.as_str()) {
        Ok(())
    } else {
        Err(ErrorAccion::Argumentos(format!(
            "servicio '{}' fuera de la lista cerrada ({})",
            a.servicio,
            SERVICIOS_REINICIABLES.join(", ")
        )))
    }
}

/// El estado de la unidad, si el servicio es de la lista cerrada.
pub(super) async fn observar<C: Consulta>(consulta: &C, args: &Value) -> Result<Option<EstadoServicio>, ErrorAccion> {
    match args["servicio"].as_str().filter(|s| SERVICIOS_REINICIABLES.contains(s)) {
        Some(srv) => Ok(Some(consulta.servicio_estado(srv).await.map_err(ErrorAccion::Sistema)?)),
        None => Ok(None),
    }
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    let estado = p.hechos.servicio.clone().ok_or_else(|| ErrorAccion::Sistema("servicio no observado".into()))?;
    if !estado.cargado {
        return Err(ErrorAccion::NoAplica(format!("la unidad {}.service no está instalada en systemd --user", a.servicio)));
    }
    Ok((
        json!({ "servicio": a.servicio }),
        format!(
            "Reiniciaría {}.service (ahora: {}). Las conexiones y trabajos en curso de ese \
             servicio se interrumpirían unos segundos.",
            a.servicio, estado.activo
        ),
        json!({ "cargado": true }),
    ))
}

pub(super) async fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    p.sistema.servicio_reiniciar(&a.servicio).await.map_err(ErrorAccion::Sistema)?;
    let ahora = p.sistema.servicio_estado(&a.servicio).await.map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada {
        texto: format!("{}.service reiniciado (ahora: {}).", a.servicio, ahora.activo),
        trabajo: None,
        datos: json!({ "servicio": a.servicio, "activo": ahora.activo }),
    })
}
