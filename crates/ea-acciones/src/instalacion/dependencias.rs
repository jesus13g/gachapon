//! Spec 023: ver el plan en la bóveda (`specs/023-instalacion/plan.md`).
// STUB B1: lo sustituye B2/B3

use ea_plataforma::Sistema;

use super::{Contexto, Sonda};
use crate::servicios::Paso;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Estado {
    Presente(String),
    Falta,
}

#[derive(Debug, Clone)]
pub struct Oferta {
    pub explicacion: String,
    pub pasos: Vec<Paso>,
    pub admin: bool,
}

#[derive(Debug, Clone)]
pub struct Dependencia {
    pub id: &'static str,
    pub descripcion: String,
    pub estado: Estado,
    pub obligatoria: bool,
    pub oferta: Option<Oferta>,
    pub nota: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestorPaquetes {
    Pacman,
    Apt,
    Dnf,
    Zypper,
    Brew,
    Winget,
}

pub fn gestor_paquetes(sistema: Sistema, sonda: &dyn Sonda) -> Option<GestorPaquetes> {
    let _ = (sistema, sonda);
    None
}

pub fn detectar(ctx: &Contexto, sonda: &dyn Sonda) -> Vec<Dependencia> {
    let _ = (ctx, sonda);
    vec![]
}

pub fn vram_gb(sistema: Sistema, sonda: &dyn Sonda) -> Option<u32> {
    let _ = (sistema, sonda);
    None
}
