//! Spec 023: ver el plan en la bóveda (`specs/023-instalacion/plan.md`).
// STUB B1: lo sustituye B2/B3

use std::path::Path;

use super::{Contexto, Sonda};
use crate::servicios::Paso;

pub fn plan_agentes(ctx: &Contexto, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let _ = (ctx, sonda);
    Ok(vec![])
}

pub fn plan_quitar_agentes(ctx: &Contexto, sonda: &dyn Sonda) -> Result<Vec<Paso>, String> {
    let _ = (ctx, sonda);
    Ok(vec![])
}

pub fn fusionar_hook(settings: Option<&str>, ea: &Path) -> Result<Option<String>, String> {
    let _ = (settings, ea);
    Ok(None)
}

pub fn quitar_hook(settings: &str) -> Result<Option<String>, String> {
    let _ = settings;
    Ok(None)
}

pub fn poner_fragmento(actual: Option<&str>, fragmento: &str) -> String {
    format!("{}{fragmento}", actual.unwrap_or_default())
}

pub fn quitar_fragmento(actual: &str) -> Option<String> {
    let _ = actual;
    None
}
