//! `ea_desprogramar_rutina`: quitar el timer de systemd de una rutina.

use serde_json::json;

use super::{comprobar_slug, Aplicada, Descripcion, ParaAplicar, ParaSimular, PorSlug};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};
use crate::temporizador::unidad_timer;

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: PorSlug) -> Result<Descripcion, ErrorAccion> {
    comprobar_slug(&a.slug)?;
    // No exige que esté registrada: también quita el timer de una
    // rutina retirada.
    let timer = p.hechos.timer.clone().ok_or_else(|| ErrorAccion::Sistema("timer no observado".into()))?;
    if !timer.programado() {
        return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no tiene timer programado", a.slug)));
    }
    let horario = timer.calendario.as_deref().map(|c| format!(" (OnCalendar={c})")).unwrap_or_default();
    Ok((
        json!({ "slug": a.slug }),
        format!(
            "Desprogramaría la rutina '{}'{horario}: `systemctl --user disable --now {}`, borra el \
             fichero del timer y daemon-reload. La rutina sigue registrada y se puede lanzar a mano \
             o volver a programar.",
            a.slug,
            unidad_timer(&a.slug)
        ),
        json!({ "calendario_actual": timer.calendario, "fichero": timer.fichero }),
    ))
}

pub(super) async fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: PorSlug) -> Result<Aplicada, ErrorAccion> {
    let habia = p.sistema.timer_desprogramar(&a.slug).await.map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada {
        texto: if habia {
            format!("Timer de la rutina '{}' desactivado y borrado.", a.slug)
        } else {
            format!("La rutina '{}' no tenía timer.", a.slug)
        },
        trabajo: None,
        datos: json!({ "rutina": a.slug, "desprogramada": habia }),
    })
}
