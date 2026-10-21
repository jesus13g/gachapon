//! `ea_lanzar_sesion` (spec 018, F5): el pegamento entre la biblioteca
//! (`ea-biblioteca::sesion`, plan puro y ficheros efímeros) y el flujo de
//! acciones (simular, proponer, aceptar, ejecutar). Aquí viven las
//! funciones que usan el sistema real, el de solo consulta y el simulado, y
//! la que llaman las interfaces (`ea`, `ea-web`).

use std::path::{Path, PathBuf};

use ea_biblioteca::{materializar, plan_sesion, Biblioteca, Id, Inventario, PlanSesion, Raices};
use serde_json::{json, Value};
use ea_store::Store;

use crate::catalogo::Canal;
use crate::error::ErrorAccion;
use crate::flujo::{ejecutar_directo, Resultado};
use crate::sistema::Sistema;
use crate::terminal::Terminal;

/// Las raíces del entorno real; `directorio` (absoluto, existente) sustituye
/// al proyecto. Sin él, el directorio actual del proceso.
pub fn raices_de(directorio: Option<&str>) -> Result<Raices, String> {
    let mut r = Raices::desde_entorno();
    if let Some(d) = directorio.map(str::trim).filter(|d| !d.is_empty()) {
        let ruta = Path::new(d);
        if !ruta.is_absolute() {
            return Err(format!("el directorio debe ser una ruta absoluta: {d}"));
        }
        let ruta: PathBuf = std::fs::canonicalize(ruta).map_err(|e| format!("el directorio {d} no existe: {e}"))?;
        if !ruta.is_dir() {
            return Err(format!("{d} no es un directorio"));
        }
        // Solo bajo el HOME: el proyecto decide qué ficheros se leen.
        let home = std::fs::canonicalize(&r.home).unwrap_or_else(|_| r.home.clone());
        if !ruta.starts_with(&home) {
            return Err(format!("el directorio {d} está fuera del HOME: no se abre una sesión ahí"));
        }
        r.proyecto = Some(ruta).filter(|p| *p != r.home && *p != home);
    }
    Ok(r)
}

/// Los ids tal como los escribe el usuario, ya analizados.
pub fn analizar_ids(ids: &[String]) -> Result<Vec<Id>, String> {
    ids.iter().map(|i| i.parse::<Id>().map_err(|e| e.to_string())).collect()
}

/// El plan sobre el inventario real (lee el disco; no escribe nada).
pub fn plan_real(agente: &str, ids: &[String], directorio: Option<&str>) -> Result<PlanSesion, String> {
    let raices = raices_de(directorio)?;
    let ajustes = ea_ajustes::cargar();
    let inventario = Biblioteca::cargar(&raices, &ajustes);
    plan_sesion(agente, &analizar_ids(ids)?, &inventario, &raices).map_err(|e| e.to_string())
}

/// Las raíces con las que se materializó el plan: el proyecto es el suyo.
fn raices_del_plan(plan: &PlanSesion, base: Raices) -> Raices {
    Raices { proyecto: plan.cwd.clone(), ..base }
}

/// Materializa y abre la terminal, con el entorno real.
pub fn lanzar_real(plan: &PlanSesion, terminal: &dyn Terminal) -> Result<(), String> {
    lanzar_con(plan, raices_del_plan(plan, Raices::desde_entorno()), terminal)
}

/// Materializa con las `raices` dadas y abre la terminal.
pub fn lanzar_con(plan: &PlanSesion, raices: Raices, terminal: &dyn Terminal) -> Result<(), String> {
    // El plan real es el devuelto: su directorio lleva un nonce propio.
    let real = materializar(plan, &raices).map_err(|e| e.to_string())?;
    terminal.abrir(&real.argv_terminal())
}

/// El plan sobre un inventario y unas raíces dados (el doble de las pruebas).
pub fn plan_en(agente: &str, ids: &[String], directorio: Option<&str>, inventario: &Inventario, raices: &Raices) -> Result<PlanSesion, String> {
    let mut r = raices.clone();
    if let Some(d) = directorio.map(str::trim).filter(|d| !d.is_empty()) {
        r.proyecto = Some(PathBuf::from(d));
    }
    plan_sesion(agente, &analizar_ids(ids)?, inventario, &r).map_err(|e| e.to_string())
}

/// La función que llaman las interfaces del usuario (CLI `ea biblioteca
/// sesion`, web): el gesto del usuario es la confirmación. Simula (el plan
/// sin secretos), revalida, materializa los ficheros 0600, abre la terminal
/// y deja `accion_ejecutada` en el diario con agente, ids y argv.
pub async fn lanzar_sesion<S: Sistema>(
    store: &mut Store,
    sistema: &S,
    actor: &str,
    canal: Canal,
    agente: &str,
    herramientas: &[String],
    directorio: Option<&str>,
) -> Result<Resultado, ErrorAccion> {
    let mut args = json!({ "agente": agente, "herramientas": herramientas });
    if let Some(d) = directorio.map(str::trim).filter(|d| !d.is_empty()) {
        args["directorio"] = Value::String(d.to_string());
    }
    ejecutar_directo(store, sistema, actor, canal, "ea_lanzar_sesion", &args).await
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn el_directorio_de_una_sesion_tiene_que_estar_bajo_el_home() {
        // `/` existe y es un directorio, pero no está bajo el HOME.
        let e = raices_de(Some("/")).unwrap_err();
        assert!(e.contains("fuera del HOME"), "{e}");
        assert!(raices_de(Some("/usr")).is_err());
    }
}
