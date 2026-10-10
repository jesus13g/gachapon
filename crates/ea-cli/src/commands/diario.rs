use anyhow::Result;
use comfy_table::Table;
use ea_store::{EventFilter, Store};

use super::{fmt_ts, parse_flexible_date};

pub fn listar(
    store: &Store,
    tarea: Option<i64>,
    actor: Option<String>,
    desde: Option<String>,
    hasta: Option<String>,
    limite: i64,
) -> Result<()> {
    let filter = EventFilter {
        task_id: tarea,
        actor,
        since: desde.as_deref().map(parse_flexible_date).transpose()?,
        until: hasta.as_deref().map(parse_flexible_date).transpose()?,
        exclude_action: None,
        limit: Some(limite),
    };

    let events = store.list_events(&filter)?;
    if events.is_empty() {
        println!("No hay eventos que coincidan con el filtro.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["ID", "Cuándo", "Actor", "Acción", "Tarea", "Resultado"]);
    for e in events {
        table.add_row(vec![
            e.id.to_string(),
            fmt_ts(e.ts),
            e.actor,
            e.action,
            e.task_id.map(|t| format!("#{t}")).unwrap_or_default(),
            e.outcome.map(|o| o.as_str().to_string()).unwrap_or_default(),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn ver(store: &Store, id: i64) -> Result<()> {
    let e = store.get_event(id)?;
    println!("Evento #{}", e.id);
    println!("  Cuándo:    {}", fmt_ts(e.ts));
    println!("  Actor:     {}", e.actor);
    println!("  Acción:    {}", e.action);
    if let Some(task_id) = e.task_id {
        println!("  Tarea:     #{task_id}");
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
