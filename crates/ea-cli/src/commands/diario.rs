use anyhow::Result;
use comfy_table::Table;
use ea_store::{EventFilter, EventOutcome, Store};

use super::{fmt_ts, parse_flexible_date};

/// Los filtros de `ea diario listar`, tal como llegan de la línea de órdenes.
pub struct Filtro {
    pub trabajo: Option<String>,
    pub actor: Option<String>,
    pub desde: Option<String>,
    pub hasta: Option<String>,
    /// `ok` o `error` (clap ya lo ha validado).
    pub resultado: Option<String>,
    /// Prefijo de la acción (clap ya lo ha validado como `[a-z_]+`).
    pub accion: Option<String>,
    /// Cursor: solo los eventos anteriores al de este id.
    pub antes_de: Option<i64>,
}

pub fn listar(store: &Store, f: Filtro, limite: i64) -> Result<()> {
    let filter = EventFilter {
        trabajo: f.trabajo,
        actor: f.actor,
        since: f.desde.as_deref().map(parse_flexible_date).transpose()?,
        until: f.hasta.as_deref().map(parse_flexible_date).transpose()?,
        outcome: f.resultado.as_deref().map(str::parse::<EventOutcome>).transpose()?,
        action_prefix: f.accion,
        before_id: f.antes_de,
        limit: Some(limite),
        ..Default::default()
    };

    let events = store.list_events(&filter)?;
    if events.is_empty() {
        println!("No hay eventos que coincidan con el filtro.");
        return Ok(());
    }

    // Si la página va llena puede haber más: el cursor de la siguiente.
    let siguiente = (events.len() as i64 >= limite).then(|| events.last().map(|e| e.id)).flatten();

    let mut table = Table::new();
    table.set_header(vec!["ID", "Cuándo", "Actor", "Acción", "Trabajo", "Resultado"]);
    for e in events {
        table.add_row(vec![
            e.id.to_string(),
            fmt_ts(e.ts),
            e.actor,
            e.action,
            e.trabajo.unwrap_or_default(),
            e.outcome.map(|o| o.as_str().to_string()).unwrap_or_default(),
        ]);
    }
    println!("{table}");
    if let Some(id) = siguiente {
        println!("Más antiguos: añade --antes-de {id}");
    }
    Ok(())
}

pub fn ver(store: &Store, id: i64) -> Result<()> {
    let e = store.get_event(id)?;
    println!("Evento #{}", e.id);
    println!("  Cuándo:    {}", fmt_ts(e.ts));
    println!("  Actor:     {}", e.actor);
    println!("  Acción:    {}", e.action);
    if let Some(trabajo) = &e.trabajo {
        println!("  Trabajo:   {trabajo}");
    }
    if let Some(outcome) = e.outcome {
        println!("  Resultado: {}", outcome.as_str());
    }
    if let Some(duration) = e.duration_ms {
        println!("  Duración:  {duration} ms");
    }
    if let Some(detail) = &e.detail {
        println!("  Detalle:   {detail}");
    }
    Ok(())
}
