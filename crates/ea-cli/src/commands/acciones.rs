use anyhow::{bail, Context, Result};
use comfy_table::Table;
use ea_acciones::{Ejecucion, CATALOGO};
use serde_json::Value;

use super::{bloquear, confirmar_en_terminal, Contexto};

const ACTOR: &str = ea_ajustes::ACTOR_USUARIO;

fn parsear(args: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(args).context("--args debe ser un objeto JSON")?;
    if !v.is_object() {
        bail!("--args debe ser un objeto JSON");
    }
    Ok(v)
}

pub fn listar() {
    let mut table = Table::new();
    table.set_header(vec!["Herramienta", "Riesgo", "Ejecución desde agentes", "Modelo local", "Qué hace"]);
    for h in CATALOGO {
        table.add_row(vec![
            h.nombre.to_string(),
            h.riesgo.as_str().to_string(),
            match h.ejecucion {
                Ejecucion::Directa => "directa".to_string(),
                Ejecucion::Supervisada => "solo propone".to_string(),
            },
            if h.local { "sí" } else { "no" }.to_string(),
            h.resumen.to_string(),
        ]);
    }
    println!("{table}");
}

pub fn simular(ctx: &mut Contexto, herramienta: &str, args: &str) -> Result<()> {
    let sis = ctx.sistema();
    let store = ctx.store()?;
    let args = parsear(args)?;
    let hechos = bloquear(ea_acciones::observar(&sis, herramienta, &args))?;
    let sim = ea_acciones::simular_con(store, &sis, &hechos, herramienta, &args)?;
    println!("Simulación de {} ({}); no se ha hecho nada:", sim.herramienta, sim.riesgo.as_str());
    println!("  {}", sim.texto);
    Ok(())
}

/// Ejecución directa por el usuario: simulación, confirmación en terminal
/// interactiva y, solo entonces, la acción por la capa interna.
pub fn ejecutar(ctx: &mut Contexto, herramienta: &str, args: &str) -> Result<()> {
    let sis = ctx.sistema();
    let store = ctx.store()?;
    let args = parsear(args)?;
    let canal = ea_acciones::terminal_interactiva()?;
    let hechos = bloquear(ea_acciones::observar(&sis, herramienta, &args))?;
    let sim = ea_acciones::simular_con(store, &sis, &hechos, herramienta, &args)?;
    if !confirmar_en_terminal(herramienta, sim.riesgo.as_str(), &sim.argumentos, &sim.texto, herramienta)? {
        println!("Cancelado: no se ha hecho nada.");
        return Ok(());
    }
    let r = bloquear(ea_acciones::ejecutar(
        store, &sis, ACTOR, canal, herramienta, &sim.argumentos, &sim.precondicion, None,
    ))?;
    println!("{}", r.texto);
    Ok(())
}
