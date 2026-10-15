//! `ea qwen-skills`: catálogo de skills del agente local y validación de las
//! que elige quien crea un trabajo.

use anyhow::{anyhow, bail, Result};
use ea_bus::{ConfigOrquestador, Orden};

pub fn listar(json: bool) -> Result<()> {
    let cfg = ConfigOrquestador::cargar().skills;
    let catalogo = cfg.catalogo()?;
    if json {
        let v: Vec<_> = catalogo
            .iter()
            .map(|s| {
                serde_json::json!({
                    "nombre": s.nombre, "descripcion": s.descripcion,
                    "caracteres": s.cuerpo.chars().count(), "habilitada": s.habilitada,
                })
            })
            .collect();
        let v = serde_json::json!({
            "catalogo": cfg.ruta_catalogo(), "max_por_trabajo": cfg.max_por_trabajo,
            "max_caracteres": cfg.max_caracteres, "pueden_elegir": cfg.pueden_elegir, "skills": v,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    println!(
        "catalogo {}  ·  max {} skills / {} caracteres  ·  pueden elegir: {}",
        cfg.ruta_catalogo().display(),
        cfg.max_por_trabajo,
        cfg.max_caracteres,
        cfg.pueden_elegir.join(", ")
    );
    if catalogo.is_empty() {
        println!("(catalogo vacio)");
    }
    for s in &catalogo {
        let marca = if s.habilitada { " " } else { "x" };
        println!("{marca} {:<14} {:>5}c  {}", s.nombre, s.cuerpo.chars().count(), s.descripcion);
    }
    Ok(())
}

pub fn ver(nombre: &str) -> Result<()> {
    let cfg = ConfigOrquestador::cargar().skills;
    let s = cfg.catalogo()?.into_iter().find(|s| s.nombre == nombre).ok_or_else(|| anyhow!("no existe la skill {nombre:?}"))?;
    println!("{}", s.cuerpo);
    Ok(())
}

pub fn probar(por: &str, skills: &[String]) -> Result<()> {
    let s = ConfigOrquestador::cargar().skills.resolver(por, skills)?;
    let total: usize = s.iter().map(|s| s.cuerpo.chars().count()).sum();
    println!("ok: {} skills, {total} caracteres", s.len());
    Ok(())
}

/// Las skills de la orden y de cada fase deben pasar la llave y los topes.
pub fn validar_orden(orden: &Orden) -> Result<()> {
    let cfg = ConfigOrquestador::cargar().skills;
    cfg.resolver(&orden.creada_por, &orden.skills)?;
    for f in &orden.fases {
        if let Err(e) = cfg.resolver(&orden.creada_por, &f.skills) {
            bail!("fase {}: {e}", f.numero);
        }
    }
    Ok(())
}
