//! Rutinas registradas y su lanzamiento manual.

use super::{bus_con_store, slug_valido, ErrorWeb, Resultado};
use crate::vistas::{RutinaDetalle, RutinaFila, RutinaLanzada};
use crate::AppState;

pub async fn listar(s: &AppState) -> Resultado<Vec<RutinaFila>> {
    let filas = s.store.lock().await.listar_rutinas()?;
    let mut out = Vec::with_capacity(filas.len());
    for f in filas {
        let r = ea_bus::Rutina::parsear(&f.definicion).ok();
        let proxima = if s.config.systemctl && ea_bus::slug_valido(&f.slug) {
            crate::estado::proxima_ejecucion(&f.slug).await
        } else {
            None
        };
        out.push(RutinaFila {
            error: r.is_none().then_some("definición ilegible"),
            descripcion: r.as_ref().map(|r| r.rutina.descripcion.clone()),
            worktree: r.as_ref().map(|r| r.rutina.worktree.clone()),
            criterio_exito: r.as_ref().map(|r| r.rutina.criterio_exito.clone()),
            entradas: r.map(|r| r.entradas),
            slug: f.slug,
            version: f.version,
            registrada: f.creada_en,
            proxima,
        });
    }
    Ok(out)
}

fn no_registrada(slug: &str) -> ErrorWeb {
    ErrorWeb::NoEncontrado(format!("la rutina '{slug}' no está registrada"))
}

pub async fn ver(s: &AppState, slug: &str) -> Resultado<RutinaDetalle> {
    slug_valido(slug)?;
    let fila = s.store.lock().await.ultima_rutina(slug)?.ok_or_else(|| no_registrada(slug))?;
    Ok(RutinaDetalle { slug: slug.into(), version: fila.version, contenido: fila.definicion })
}

/// Lo mismo que hace el disparador de systemd (`ea rutina lanzar`): crea el
/// trabajo en el bus y el orquestador lo recoge.
pub async fn lanzar(s: &AppState, slug: &str) -> Resultado<RutinaLanzada> {
    slug_valido(slug)?;
    let bus = bus_con_store(s).await?;
    bus.rutina(slug).map_err(|_| no_registrada(slug))?;
    if !bus.trabajos_activos_de_rutina(slug)?.is_empty() {
        return Err(ErrorWeb::Conflicto(format!("la rutina '{slug}' ya tiene una ejecución en curso")));
    }
    let id = bus.lanzar_rutina(slug, &[])?;
    Ok(RutinaLanzada { trabajo: id, rutina: slug.into() })
}
