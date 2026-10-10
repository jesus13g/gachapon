use anyhow::{bail, Result};
use comfy_table::Table;
use ea_store::{EstadoPropuesta, PropuestaFilter, Store};

use super::fmt_ts;

pub fn listar(store: &Store, estado: Option<EstadoPropuesta>) -> Result<()> {
    let propuestas = store.list_propuestas(&PropuestaFilter { estado, limit: Some(50) })?;

    if propuestas.is_empty() {
        println!("No hay propuestas que coincidan con el filtro.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["ID", "Ejecución", "Estado", "Creada", "Descripción"]);
    for p in propuestas {
        table.add_row(vec![
            p.id.to_string(),
            p.routine_run_id.to_string(),
            p.estado.as_str().to_string(),
            fmt_ts(p.creado_en),
            p.descripcion,
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn ver(store: &Store, id: i64) -> Result<()> {
    let p = store.get_propuesta(id)?;
    let run = store.get_routine_run(p.routine_run_id)?;

    println!("Propuesta #{}", p.id);
    println!("  Rutina:      {}", run.routine_slug);
    println!("  Ejecución:   #{}", run.id);
    println!("  Estado:      {}", p.estado.as_str());
    println!("  Creada:      {}", fmt_ts(p.creado_en));
    if let Some(decidido_en) = p.decidido_en {
        println!("  Decidida:    {}", fmt_ts(decidido_en));
    }
    println!("  Descripción: {}", p.descripcion);
    Ok(())
}

/// Registra la decisión del usuario. No crea ninguna tarea: ejecutar una
/// propuesta aceptada requiere una herramienta mutante, que no existe
/// todavía (fase 6) — esto solo deja constancia de la decisión, igual que
/// `ea tareas actualizar` ya escribe directo sobre el estado de una tarea.
pub fn decidir(store: &mut Store, id: i64, aceptar: bool, rechazar: bool) -> Result<()> {
    let estado = match (aceptar, rechazar) {
        (true, false) => EstadoPropuesta::Aceptada,
        (false, true) => EstadoPropuesta::Rechazada,
        _ => bail!("especifica exactamente una de --aceptar o --rechazar"),
    };
    let p = store.decidir_propuesta(id, estado)?;
    println!("Propuesta #{} marcada como '{}'", p.id, p.estado.as_str());
    Ok(())
}
