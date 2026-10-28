//! `ea_activar_herramienta` y `ea_desactivar_herramienta` (spec 018, F4):
//! activar o desactivar un elemento de la biblioteca; la una deshace la otra.

use ea_biblioteca::{Cambio, Id};
use ea_store::{EventOutcome, NewEvent};
use serde::Deserialize;
use serde_json::json;

use super::{campo, Aplicada, Campo, Descripcion, ParaAplicar, ParaSimular};
use crate::error::ErrorAccion;
use crate::sistema::{Consulta, Sistema};

pub(super) const CAMPOS: &[Campo] =
    &[campo("elemento", "id del elemento de la biblioteca (agente/tipo/ámbito/nombre, p. ej. codex/mcp/usuario/docs)")];

/// El id de un elemento de la biblioteca (`agente/tipo/ámbito/nombre`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Args {
    elemento: String,
}

/// Solo ids con la forma del inventario, nunca texto libre.
pub(super) fn validar(a: &Args) -> Result<(), ErrorAccion> {
    Id::analizar(&a.elemento).map(|_| ()).map_err(|e| ErrorAccion::Argumentos(e.to_string()))
}

pub(super) fn simular<C: Consulta>(p: &ParaSimular<'_, C>, a: Args, activar: bool) -> Result<Descripcion, ErrorAccion> {
    validar(&a)?;
    let cambio = p.consulta.biblioteca_cambio(&a.elemento, activar).map_err(ErrorAccion::NoAplica)?;
    // El cambio entero es la precondición: al aceptar se recalcula y debe ser idéntico
    // (incluida la huella del fichero que se editaría).
    let precondicion = json!({ "cambio": cambio });
    let deshace = if activar { "desactivarlo" } else { "activarlo" };
    Ok((json!({ "elemento": a.elemento }), format!("{} Es reversible: {deshace} lo deshace.", cambio.describir()), precondicion))
}

pub(super) async fn aplicar<S: Sistema>(p: ParaAplicar<'_, S>, a: Args, activar: bool) -> Result<Aplicada, ErrorAccion> {
    // Lo que aceptó el usuario es el cambio simulado; la biblioteca lo revalida
    // contra el estado de ahora y rechaza el que no coincida con lo que habría hecho.
    let cambio: Cambio = serde_json::from_value(p.precondicion["cambio"].clone())
        .map_err(|_| ErrorAccion::NoAplica("la propuesta no lleva el cambio simulado".into()))?;
    if cambio.id != a.elemento || cambio.activar != activar {
        return Err(ErrorAccion::NoAplica("el cambio simulado no es el de esta herramienta y este elemento".into()));
    }
    let h = p.sistema.biblioteca_aplicar(&cambio, p.canal).await.map_err(ErrorAccion::Sistema)?;
    let datos = json!({
        "elemento": h.id, "activar": h.activar, "mecanismo": h.mecanismo, "objetivo": h.objetivo,
        "antes": h.antes, "despues": h.despues, "copia": h.copia,
    });
    // Un evento propio con el valor anterior (art. VI): el de la acción lo lleva también en `resultado`.
    let _ = p.store.append_event(NewEvent {
        actor: p.actor.to_string(),
        action: "biblioteca_cambio".to_string(),
        outcome: Some(EventOutcome::Ok),
        detail: Some(datos.to_string()),
        ..Default::default()
    });
    Ok(Aplicada {
        texto: format!("{} {}: {}.", if activar { "Activado" } else { "Desactivado" }, h.id, h.texto),
        trabajo: None,
        datos,
    })
}
