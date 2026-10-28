//! `ea_encolar_rutina`: lanzar una rutina registrada por el bus (crea el
//! trabajo; lo ejecuta el agente local).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;

use super::{bus_de, comprobar_slug, exigir_lanzable, Aplicada, Descripcion, ParaAplicar, ParaSimular};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};

/// La rutina y, si se quieren cambiar, sus entradas (`{clave: valor}`, con la
/// política de `ea_bus::validar_entradas`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    slug: String,
    #[serde(default)]
    entradas: BTreeMap<String, String>,
}

impl Args {
    fn pares(&self) -> Vec<(String, String)> {
        self.entradas.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

/// Validación sin mirar el mundo: slug y límites de las entradas.
pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    comprobar_slug(&a.slug)?;
    ea_bus::validar_entradas(&a.pares()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    // Retirada o sin fichero: no se lanza (el mismo texto que la web y la CLI).
    exigir_lanzable(p.consulta, &a.slug)?;
    let version = p
        .store
        .ultima_rutina(&a.slug)?
        .ok_or_else(|| ErrorAccion::NoAplica(format!("la rutina '{}' no está registrada (ea rutina registrar)", a.slug)))?
        .version;
    let bus = bus_de(&p.consulta.estado_dir(), p.store, "simulacion")?;
    // La orden que saldría, con las mismas comprobaciones que al
    // lanzar (entrada no declarada, límites): así el usuario ve el
    // criterio que ejecutará el sistema antes de aceptar.
    let orden = bus
        .rutina(&a.slug)
        .and_then(|r| r.a_orden(&a.pares()))
        .map_err(|e| ErrorAccion::Argumentos(e.to_string()))?;
    // Un worktree que ya no vale (U-05) no se lanzaría: se dice ahora.
    ea_bus::validar_worktree(&orden.worktree, ea_bus::git::TOPE_LECTURA).map_err(|e| ErrorAccion::NoAplica(e.to_string()))?;
    let activos = bus.trabajos_activos_de_rutina(&a.slug).map_err(|e| ErrorAccion::Sistema(e.to_string()))?;
    if let Some(t) = activos.first() {
        return Err(ErrorAccion::NoAplica(format!("la rutina '{}' ya tiene una ejecución en curso (trabajo {t})", a.slug)));
    }
    let entradas = if a.entradas.is_empty() {
        "con las entradas por defecto".to_string()
    } else {
        let lista: Vec<String> = a.entradas.iter().map(|(k, v)| format!("{k} = {v:?}")).collect();
        format!("con las entradas {}", lista.join(", "))
    };
    // Sin entradas, los argumentos quedan como antes ({slug}).
    let argumentos =
        if a.entradas.is_empty() { json!({ "slug": a.slug }) } else { json!({ "slug": a.slug, "entradas": a.entradas }) };
    Ok((
        argumentos,
        format!(
            "Lanzaría la rutina '{}' (versión {version}) {entradas} por el bus del orquestador: crea un \
             trabajo para el agente local, que la ejecuta y solo vuelve a Claude Code si falla \
             y tú lo confirmas. Criterio que ejecutará el sistema: `{}`.",
            a.slug,
            orden.criterio_exito.as_deref().unwrap_or_default()
        ),
        json!({ "trabajos_activos": 0, "version": version }),
    ))
}

pub(super) fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args) -> Result<Aplicada, ErrorAccion> {
    let id = p.sistema.rutina_lanzar(p.store, &a.slug, &a.pares(), p.actor)?;
    Ok(Aplicada {
        texto: format!("Rutina '{}' lanzada: trabajo {id} en la cola del agente local.", a.slug),
        trabajo: Some(id.clone()),
        datos: json!({ "trabajo": id, "rutina": a.slug }),
    })
}
