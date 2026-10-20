//! Rutinas registradas y su lanzamiento manual.

use std::collections::{BTreeMap, HashMap, HashSet};

use ea_bus::{Bus, Rutina};
use ea_store::{EventOutcome, NewEvent};
use serde_json::json;

use super::{bus_con_store, slug_valido, ErrorWeb, Resultado};
use crate::vistas::{RutinaAnterior, RutinaDetalle, RutinaFila, RutinaLanzada, RutinaPrevia, RutinaRegistrada, RutinaValidada};
use crate::AppState;

/// Cuántas ejecuciones lleva la ficha de una rutina.
const EJECUCIONES_EN_FICHA: usize = 10;

/// La ejecución `id` de una rutina tal como la ve la web: estado, versión
/// de la rutina (la de su `orden.json`) y, si entregó informe, resultado y
/// duración. `None` si el trabajo no se puede leer.
fn ejecucion(bus: &Bus, id: &str) -> Option<crate::vistas::RutinaEjecucion> {
    let orden = bus.leer_orden(id).ok()?;
    let estado = bus.leer_estado(id).ok()?;
    let informe = bus.leer_informe(id).ok().flatten();
    Some(crate::vistas::RutinaEjecucion {
        trabajo: id.into(),
        estado: estado.estado.as_str(),
        version: orden.rutina.map_or(0, |r| r.version),
        resultado: informe.as_ref().map(|i| match i.resultado {
            ea_bus::ResultadoInforme::Terminado => "terminado",
            ea_bus::ResultadoInforme::Bloqueado => "bloqueado",
        }),
        duracion_s: informe.map(|i| i.duracion_s),
        detalle: estado.detalle,
        creado: orden.creada_en,
        actualizado: estado.actualizado_en,
    })
}

/// El trabajo más reciente de cada una de `slugs`, en una sola pasada por
/// el bus del más nuevo al más viejo (los ids empiezan por la fecha): se lee
/// la orden de cada trabajo hasta haber visto todas, y el estado y el
/// informe solo del elegido. Si el bus no se puede abrir, ninguna.
fn ultimas(s: &AppState, slugs: &HashSet<String>) -> HashMap<String, crate::vistas::RutinaEjecucion> {
    let mut out = HashMap::new();
    let Ok(bus) = super::bus_lectura(s) else { return out };
    let Ok(ids) = bus.listar_trabajos() else { return out };
    for id in ids.iter().rev() {
        if out.len() == slugs.len() {
            break;
        }
        let Some(slug) = bus.leer_orden(id).ok().and_then(|o| o.rutina).map(|r| r.slug) else { continue };
        if slugs.contains(&slug) && !out.contains_key(&slug) {
            if let Some(e) = ejecucion(&bus, id) {
                out.insert(slug, e);
            }
        }
    }
    out
}

pub async fn listar(s: &AppState) -> Resultado<Vec<RutinaFila>> {
    let filas = s.store.lock().await.listar_rutinas()?;
    let mut ultimas = ultimas(s, &filas.iter().map(|f| f.slug.clone()).collect());
    let mut out = Vec::with_capacity(filas.len());
    for f in filas {
        let r = ea_bus::Rutina::parsear(&f.definicion).ok();
        let proxima = if s.config.systemctl && ea_bus::slug_valido(&f.slug) {
            crate::estado::proxima_ejecucion(&f.slug).await
        } else {
            None
        };
        let ultima = ultimas.remove(&f.slug);
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
            ultima,
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
    // Como `ea rutina ver`: los trabajos de la rutina, el más reciente primero.
    let ejecuciones = match super::bus_lectura(s) {
        Ok(bus) => bus
            .trabajos_de_rutina(slug)?
            .into_iter()
            .take(EJECUCIONES_EN_FICHA)
            .filter_map(|(id, _)| ejecucion(&bus, &id))
            .collect(),
        Err(_) => Vec::new(),
    };
    Ok(RutinaDetalle { slug: slug.into(), version: fila.version, contenido: fila.definicion, ejecuciones })
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

/// Lo que registraría `toml` (RF-017 de la spec 004): las mismas
/// comprobaciones que `ea rutina registrar` (`ea_bus::comprobar_registro`:
/// tamaño, formato, versión mayor que la registrada) y la vista previa de la
/// orden con las entradas por defecto. No escribe nada ni ejecuta el criterio.
pub async fn validar(s: &AppState, toml: &str) -> Resultado<RutinaValidada> {
    let (r, anterior) = ea_bus::comprobar_registro(&*s.store.lock().await, toml)?;
    let previa = previa_de(&r, &Entradas::new())?;
    Ok(RutinaValidada {
        rutina: r.rutina.slug.clone(),
        version: r.rutina.version,
        anterior: anterior.map(|f| RutinaAnterior { version: f.version, definicion: f.definicion }),
        previa,
    })
}

/// Registra `toml` igual que `ea rutina registrar`: `Bus::registrar_rutina`
/// guarda la versión en SQLite (con `creada_por` el usuario) y copia el TOML
/// a `rutinas_dir/<slug>.toml`. Si no vale, el error del bus y nada escrito.
/// Deja `rutina_registrada` en el diario, como las demás decisiones de la web.
pub async fn registrar(s: &AppState, toml: &str) -> Resultado<RutinaRegistrada> {
    let anterior = ea_bus::comprobar_registro(&*s.store.lock().await, toml)?.1.map(|f| f.version);
    let r = bus_con_store(s).await?.registrar_rutina(toml, &s.config.rutinas_dir)?;
    let c = &r.rutina;
    let evento = NewEvent {
        actor: super::ACTOR.into(),
        action: "rutina_registrada".into(),
        trabajo: None,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(
            json!({ "canal": "web", "rutina": c.slug, "version": c.version, "anterior": anterior, "bytes": toml.len() })
                .to_string(),
        ),
    };
    // El registro ya está hecho: si el diario falla, se avisa en el log.
    if let Err(e) = s.store.lock().await.append_event(evento) {
        tracing::warn!(error = %e, "no se pudo apuntar el registro de la rutina en el diario");
    }
    Ok(RutinaRegistrada { rutina: c.slug.clone(), version: c.version, anterior })
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
