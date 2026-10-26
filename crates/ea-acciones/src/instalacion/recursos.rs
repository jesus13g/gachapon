//! Spec 023: ver el plan en la bóveda (`specs/023-instalacion/plan.md`).
// STUB B1: lo sustituye B2/B3

use std::path::Path;

/// Un fichero del repo embebido en `ea`.
pub struct Recurso {
    /// Ruta relativa a la raíz del repo.
    pub ruta: &'static str,
    pub contenido: &'static str,
}

pub const RECURSOS: &[Recurso] = &[];
pub const ICONO_PNG: &[u8] = &[];
pub const RUTA_FLUJO_FIJA: &str = "~/Proyectos/ecosistema-asistente/agentes/flujo/";

pub fn reescribir_ruta_flujo(texto: &str, flujo: &Path) -> String {
    let _ = flujo;
    texto.to_string()
}
