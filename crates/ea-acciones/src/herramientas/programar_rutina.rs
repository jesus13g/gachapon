//! `ea_programar_rutina`: el timer de systemd de una rutina registrada
//! (spec 004, H5). Se deshace con `ea_desprogramar_rutina`.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{campo, comprobar_slug, exigir_lanzable, Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};
use crate::temporizador::{comprobar_calendario, unidad_timer, Calendario};

pub(super) const CAMPOS: &[Campo] = &[
    campo("slug", "rutina"),
    campo("calendario", "OnCalendar de systemd (p. ej. *-*-* 08:00:00 o Mon *-*-* 09:00)"),
];

/// La rutina y su `OnCalendar`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    slug: String,
    calendario: String,
}

/// Validación sin mirar el mundo: slug y texto del calendario (sin
/// caracteres de control, `%` ni `\`). Devuelve el calendario recortado.
fn calendario(a: &Args) -> Result<&str, ErrorAccion> {
    comprobar_slug(&a.slug)?;
    comprobar_calendario(&a.calendario).map_err(ErrorAccion::Argumentos)
}

pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    calendario(a).map(|_| ())
}

/// Lo que dice `systemd-analyze calendar` del calendario, si su texto vale.
pub(super) async fn observar<C: Consulta>(
    consulta: &C,
    args: &Value,
) -> Result<Option<Result<Calendario, String>>, ErrorAccion> {
    match args["calendario"].as_str().filter(|c| comprobar_calendario(c).is_ok()) {
        Some(c) => Ok(Some(consulta.calendario_validar(c).await.map_err(ErrorAccion::Sistema)?)),
        None => Ok(None),
    }
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    let c = calendario(&a)?.to_string();
    // Una rutina retirada no se programa: el timer la lanzaría y
    // `ea rutina lanzar` la rechazaría.
    exigir_lanzable(p.consulta, &a.slug)?;
    // Solo las rutinas registradas: el timer lanza la última versión.
    let version = p
        .store
        .ultima_rutina(&a.slug)?
        .ok_or_else(|| ErrorAccion::NoAplica(format!("la rutina '{}' no está registrada (ea rutina registrar)", a.slug)))?
        .version;
    let cal = match &p.hechos.calendario {
        Some(Ok(cal)) => cal.clone(),
        Some(Err(m)) => return Err(ErrorAccion::Argumentos(format!("calendario no válido: {m}"))),
        None => return Err(ErrorAccion::Sistema("calendario no observado".into())),
    };
    let timer = p.hechos.timer.clone().ok_or_else(|| ErrorAccion::Sistema("timer no observado".into()))?;
    let igual = timer.calendario.as_deref().is_some_and(|t| t == c || t == cal.normalizado);
    if igual && timer.activo.as_deref() == Some("active") {
        return Err(ErrorAccion::NoAplica(format!("la rutina '{}' ya está programada con OnCalendar={c}", a.slug)));
    }
    let ahora = match (&timer.calendario, timer.activo.as_deref()) {
        (Some(t), Some(act)) => format!("Ahora está programada con OnCalendar={t} ({act}); se sustituye."),
        (Some(t), None) => format!("Ahora está programada con OnCalendar={t}; se sustituye."),
        (None, _) => "Ahora no tiene horario.".to_string(),
    };
    let proximas = if cal.proximas.is_empty() {
        "systemd no prevé ninguna ejecución".to_string()
    } else {
        format!("próximas ejecuciones: {}", cal.proximas.join("; "))
    };
    Ok((
        json!({ "slug": a.slug, "calendario": c }),
        format!(
            "Programaría la rutina '{}' (versión {version}) con OnCalendar={c} (para systemd: {}); \
             {proximas}. Escribe {} en el directorio de unidades de systemd --user, hace \
             daemon-reload y lo activa (enable y restart). A cada hora, ea-rutina@{}.service solo \
             crea el trabajo (`ea rutina lanzar {}`), con la última versión registrada; además se \
             dispara una vez 10 min después de arrancar la sesión, como la plantilla. {ahora}",
            a.slug,
            cal.normalizado,
            unidad_timer(&a.slug),
            a.slug,
            a.slug
        ),
        json!({ "calendario_actual": timer.calendario, "fichero": timer.fichero }),
    ))
}

pub(super) async fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    let c = calendario(&a)?.to_string();
    p.sistema.timer_programar(&a.slug, &c).await.map_err(ErrorAccion::Sistema)?;
    let ahora = p.sistema.timer_estado(&a.slug).await.map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada {
        texto: format!(
            "Rutina '{}' programada con OnCalendar={c} ({}).",
            a.slug,
            ahora.activo.as_deref().unwrap_or("estado desconocido")
        ),
        trabajo: None,
        datos: json!({ "rutina": a.slug, "calendario": c, "activo": ahora.activo }),
    })
}
