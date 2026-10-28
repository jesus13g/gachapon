use anyhow::{bail, Result};
use comfy_table::Table;
use ea_acciones::Canal;
use ea_store::{EstadoPropuesta, Propuesta, PropuestaFilter, Store};

use super::{bloquear, confirmar_en_terminal, fmt_ts, limpio, sistema};

const ACTOR: &str = ea_ajustes::ACTOR_USUARIO;

fn resumen_accion(p: &Propuesta) -> String {
    match &p.accion {
        Some(a) => format!("{} [{}]", a.herramienta, a.riesgo),
        None => "(solo texto)".to_string(),
    }
}

fn resumen_ejecucion(p: &Propuesta) -> String {
    p.ejecucion.as_ref().map(|e| e.estado.as_str().to_string()).unwrap_or_default()
}

pub fn listar(store: &Store, estado: Option<EstadoPropuesta>) -> Result<()> {
    let propuestas = store.list_propuestas(&PropuestaFilter { estado, limit: Some(50) })?;

    if propuestas.is_empty() {
        println!("No hay propuestas que coincidan con el filtro.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["ID", "Origen", "Estado", "Acción", "Ejecución", "Creada", "Descripción"]);
    for p in propuestas {
        table.add_row(vec![
            p.id.to_string(),
            limpio(&p.proponente.clone().unwrap_or_default()),
            p.estado.as_str().to_string(),
            resumen_accion(&p),
            limpio(&resumen_ejecucion(&p)),
            fmt_ts(p.creado_en),
            limpio(&p.descripcion),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn ver(store: &Store, id: i64) -> Result<()> {
    let p = store.get_propuesta(id)?;

    // La descripción, el proponente y la simulación los escribe un agente: sin
    // secuencias de control en la terminal.
    println!("Propuesta #{}", p.id);
    if let Some(origen) = &p.proponente {
        println!("  Propuesta por: {}", limpio(origen));
    }
    println!("  Estado:      {}", p.estado.as_str());
    println!("  Creada:      {}", fmt_ts(p.creado_en));
    if let Some(decidido_en) = p.decidido_en {
        println!("  Decidida:    {}", fmt_ts(decidido_en));
    }
    println!("  Descripción: {}", limpio(&p.descripcion));
    if let Some(a) = &p.accion {
        println!("  Acción:      {} (riesgo: {})", limpio(&a.herramienta), limpio(&a.riesgo));
        println!("  Argumentos:  {}", a.argumentos);
        println!("  Simulación:  {}", limpio(&a.simulacion));
    }
    if let Some(e) = &p.ejecucion {
        println!("  Ejecución:   {} {}", e.estado.as_str(), limpio(&e.detalle.clone().unwrap_or_default()));
    }
    Ok(())
}

/// Decide una propuesta. Rechazar y aceptar una de solo texto dejan
/// constancia. Aceptar una con acción la ejecuta, y para eso: terminal
/// interactiva real (un agente que lance `ea` sin TTY no puede confirmar),
/// simulación a la vista y confirmación explícita; las irreversibles piden
/// escribir `confirmo <id>`. Solo desde aquí se confirma una irreversible.
pub fn decidir(store: &mut Store, id: i64, aceptar: bool, rechazar: bool) -> Result<()> {
    let sis = sistema();
    match (aceptar, rechazar) {
        (false, true) => {
            let p = ea_acciones::rechazar_propuesta(store, id, ACTOR, Canal::Terminal)?;
            println!("Propuesta #{} marcada como '{}'", p.id, p.estado.as_str());
            Ok(())
        }
        (true, false) => {
            let previa = store.get_propuesta(id)?;
            let mut canal = Canal::Terminal;
            if let Some(a) = &previa.accion {
                if previa.estado != EstadoPropuesta::Pendiente {
                    bail!("propuesta #{id} ya decidida");
                }
                canal = ea_acciones::terminal_interactiva()?;
                println!("Propuesta #{id}: {}", limpio(&previa.descripcion));
                if !confirmar_en_terminal(&a.herramienta, &a.riesgo, &a.argumentos, &a.simulacion, &id.to_string())? {
                    println!("Cancelado: la propuesta sigue pendiente y no se ha hecho nada.");
                    return Ok(());
                }
            }
            let p = bloquear(ea_acciones::aceptar_propuesta(store, &sis, id, ACTOR, canal))?;
            println!("Propuesta #{} marcada como '{}'", p.id, p.estado.as_str());
            if let Some(e) = &p.ejecucion {
                println!("  Ejecución: {} — {}", e.estado.as_str(), limpio(&e.detalle.clone().unwrap_or_default()));
            }
            Ok(())
        }
        _ => bail!("especifica exactamente una de --aceptar o --rechazar"),
    }
}
