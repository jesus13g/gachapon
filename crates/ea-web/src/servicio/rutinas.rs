//! Rutinas registradas y su lanzamiento manual.

use std::collections::BTreeMap;

use ea_bus::Rutina;

use super::{bus_con_store, slug_valido, ErrorWeb, Resultado};
use crate::vistas::{RutinaDetalle, RutinaFila, RutinaLanzada, RutinaPrevia};
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

/// Entradas tal como llegan de la petición (`{clave: valor}`), en pares.
pub type Entradas = BTreeMap<String, String>;

fn pares(entradas: &Entradas) -> Vec<(String, String)> {
    entradas.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// La orden que recibiría el agente si se lanzase `rutina` con `entradas`:
/// es exactamente `Rutina::a_orden` (mismas comprobaciones: entrada no
/// declarada, límites), sin crear trabajo ni ejecutar el criterio. Vale
/// también para una definición que aún no está registrada.
pub fn previa_de(rutina: &Rutina, entradas: &Entradas) -> Resultado<RutinaPrevia> {
    let orden = rutina.a_orden(&pares(entradas))?;
    Ok(RutinaPrevia {
        rutina: rutina.rutina.slug.clone(),
        version: rutina.rutina.version,
        objetivo: orden.objetivo,
        criterio: orden.criterio_exito.unwrap_or_default(),
    })
}

/// Última versión registrada de una rutina, ya interpretada.
async fn registrada(s: &AppState, slug: &str) -> Resultado<Rutina> {
    slug_valido(slug)?;
    let fila = s.store.lock().await.ultima_rutina(slug)?.ok_or_else(|| no_registrada(slug))?;
    Ok(Rutina::parsear(&fila.definicion)?)
}

/// Vista previa de la última versión registrada. No escribe nada.
pub async fn previa(s: &AppState, slug: &str, entradas: &Entradas) -> Resultado<RutinaPrevia> {
    previa_de(&registrada(s, slug).await?, entradas)
}

/// Lo mismo que hace el disparador de systemd (`ea rutina lanzar`): crea el
/// trabajo en el bus y el orquestador lo recoge. Las entradas se validan
/// antes de mirar si hay otra ejecución en curso, y si no valen no se crea
/// nada.
pub async fn lanzar(s: &AppState, slug: &str, entradas: &Entradas) -> Resultado<RutinaLanzada> {
    slug_valido(slug)?;
    let bus = bus_con_store(s).await?;
    let orden = bus.rutina(slug).map_err(|_| no_registrada(slug))?.a_orden(&pares(entradas))?;
    if !bus.trabajos_activos_de_rutina(slug)?.is_empty() {
        return Err(ErrorWeb::Conflicto(format!("la rutina '{slug}' ya tiene una ejecución en curso")));
    }
    let id = bus.crear_trabajo(orden)?;
    Ok(RutinaLanzada { trabajo: id, rutina: slug.into() })
}
