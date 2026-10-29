//! La papelera de las rutinas (spec 004, RF-011): `ea_retirar_rutina` mueve
//! su TOML a la papelera de `rutinas_dir` (y desprograma su timer),
//! `ea_restaurar_rutina` lo devuelve y `ea_eliminar_rutina` borra para
//! siempre la copia retirada, lo único irreversible.

use serde_json::json;

use super::{Aplicada, Descripcion, ParaAplicar, ParaSimular, PorSlug, bus_de, comprobar_slug, huella, ubicacion};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};
use crate::temporizador::unidad_timer;

pub(super) fn simular_retirar<C: Consulta>(p: &ParaSimular<'_, C>, a: PorSlug) -> Result<Descripcion, ErrorAccion> {
    comprobar_slug(&a.slug)?;
    let contenido = match ubicacion(p.consulta, &a.slug)? {
        (ea_bus::Ubicacion::Activa, Some(c)) => c,
        (ea_bus::Ubicacion::Retirada, _) => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' ya está retirada", a.slug))),
        _ => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no tiene fichero en rutinas_dir", a.slug))),
    };
    let version = p.store.ultima_rutina(&a.slug)?.map(|f| format!(" (versión {} registrada)", f.version)).unwrap_or_else(|| " (no registrada)".to_string());
    let timer = p.hechos.timer.clone().ok_or_else(|| ErrorAccion::Sistema("timer no observado".into()))?;
    let del_timer = if timer.programado() {
        let horario = timer.calendario.as_deref().map(|c| format!(" (OnCalendar={c})")).unwrap_or_default();
        format!(
            "Antes desprograma su timer{horario}: `systemctl --user disable --now {}`, borra el fichero \
             del timer y daemon-reload.",
            unidad_timer(&a.slug)
        )
    } else {
        "No tiene timer que desprogramar.".to_string()
    };
    let en_curso =
        bus_de(&p.consulta.estado_dir(), p.store, "simulacion")?.trabajos_activos_de_rutina(&a.slug).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
    let en_curso = match en_curso.first() {
        Some(t) => format!(" El trabajo {t}, ya creado, sigue su curso: retirar no lo cancela."),
        None => String::new(),
    };
    Ok((
        json!({ "slug": a.slug }),
        format!(
            "Retiraría la rutina '{}'{version}: mueve rutinas_dir/{}.toml ({} bytes) a la papelera \
             (rutinas_dir/{}/{}.toml). Deja de listarse y de poder lanzarse (CLI, web, \
             ea_encolar_rutina y el timer). {del_timer} Sus versiones siguen en SQLite. Se deshace \
             con ea_restaurar_rutina, que no vuelve a programar el timer.{en_curso}",
            a.slug,
            a.slug,
            contenido.len(),
            ea_bus::DIR_PAPELERA,
            a.slug
        ),
        json!({ "huella": huella(&contenido), "calendario_actual": timer.calendario, "fichero": timer.fichero }),
    ))
}

pub(super) async fn aplicar_retirar<S: Sistema>(p: ParaAplicar<'_, S>, a: PorSlug) -> Result<Aplicada, ErrorAccion> {
    // El timer primero: si systemd falla, la rutina sigue como estaba
    // (activa y con su horario) en vez de quedar retirada con un timer
    // que la intenta lanzar.
    let habia = p
        .sistema
        .timer_desprogramar(&a.slug)
        .await
        .map_err(|e| ErrorAccion::Sistema(format!("no se pudo quitar el timer; la rutina no se ha retirado: {e}")))?;
    p.sistema
        .rutina_retirar(&a.slug)
        .map_err(|e| ErrorAccion::Sistema(if habia { format!("timer quitado, pero la rutina no se pudo retirar: {e}") } else { e }))?;
    Ok(Aplicada {
        texto: format!("Rutina '{}' retirada (su TOML está en la papelera){}.", a.slug, if habia { " y su timer desprogramado" } else { "" }),
        trabajo: None,
        datos: json!({ "rutina": a.slug, "desprogramada": habia }),
    })
}

pub(super) fn simular_restaurar<C: Consulta>(p: &ParaSimular<'_, C>, a: PorSlug) -> Result<Descripcion, ErrorAccion> {
    comprobar_slug(&a.slug)?;
    let contenido = match ubicacion(p.consulta, &a.slug)? {
        (ea_bus::Ubicacion::Retirada, Some(c)) => c,
        (ea_bus::Ubicacion::Activa, _) => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no está retirada", a.slug))),
        _ => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no está en la papelera", a.slug))),
    };
    let lanzable = match p.store.ultima_rutina(&a.slug)? {
        Some(f) => format!("vuelve a listarse y a poder lanzarse con la última versión registrada ({})", f.version),
        None => "vuelve a listarse, pero no está registrada: para lanzarla hay que registrarla".to_string(),
    };
    Ok((
        json!({ "slug": a.slug }),
        format!(
            "Restauraría la rutina '{}': devuelve rutinas_dir/{}/{}.toml ({} bytes) a rutinas_dir/{}.toml; \
             {lanzable}. No vuelve a programar su timer: si lo quieres, prográmalo después \
             (ea_programar_rutina).",
            a.slug,
            ea_bus::DIR_PAPELERA,
            a.slug,
            contenido.len(),
            a.slug
        ),
        json!({ "huella": huella(&contenido) }),
    ))
}

pub(super) fn aplicar_restaurar<S: Sistema>(p: ParaAplicar<'_, S>, a: PorSlug) -> Result<Aplicada, ErrorAccion> {
    p.sistema.rutina_restaurar(&a.slug).map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada { texto: format!("Rutina '{}' restaurada (sin timer: prográmala si lo quieres).", a.slug), trabajo: None, datos: json!({ "rutina": a.slug }) })
}

pub(super) fn simular_eliminar<C: Consulta>(p: &ParaSimular<'_, C>, a: PorSlug) -> Result<Descripcion, ErrorAccion> {
    comprobar_slug(&a.slug)?;
    // Solo se borra para siempre lo que ya está en la papelera: el
    // paso reversible (retirar) va antes, como en la bóveda.
    let contenido = match ubicacion(p.consulta, &a.slug)? {
        (ea_bus::Ubicacion::Retirada, Some(c)) => c,
        (ea_bus::Ubicacion::Activa, _) => {
            return Err(ErrorAccion::NoAplica(format!(
                "la rutina '{}' está activa: retírala antes (ea_retirar_rutina); solo se borra para \
                 siempre una rutina retirada",
                a.slug
            )));
        }
        _ => return Err(ErrorAccion::NoAplica(format!("la rutina '{}' no está en la papelera", a.slug))),
    };
    Ok((
        json!({ "slug": a.slug }),
        format!(
            "Borraría PARA SIEMPRE la copia retirada de la rutina '{}' (rutinas_dir/{}/{}.toml, {} bytes, \
             {} líneas): ese fichero no se puede recuperar. Sus versiones registradas siguen en SQLite \
             como historial; registrar una versión nueva la vuelve a poner en uso.",
            a.slug,
            ea_bus::DIR_PAPELERA,
            a.slug,
            contenido.len(),
            contenido.lines().count()
        ),
        json!({ "huella": huella(&contenido), "bytes": contenido.len() }),
    ))
}

pub(super) fn aplicar_eliminar<S: Sistema>(p: ParaAplicar<'_, S>, a: PorSlug) -> Result<Aplicada, ErrorAccion> {
    p.sistema.rutina_eliminar(&a.slug).map_err(ErrorAccion::Sistema)?;
    Ok(Aplicada { texto: format!("Copia retirada de la rutina '{}' borrada para siempre.", a.slug), trabajo: None, datos: json!({ "rutina": a.slug }) })
}
