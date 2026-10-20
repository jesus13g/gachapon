use anyhow::Result;
use comfy_table::Table;
use ea_store::{EventFilter, EventOutcome, NuevaNota, Store};

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
}

pub fn listar(store: &Store, f: Filtro, limite: i64) -> Result<()> {
    let filter = EventFilter {
        trabajo: f.trabajo,
        actor: f.actor,
        since: f.desde.as_deref().map(parse_flexible_date).transpose()?,
        until: f.hasta.as_deref().map(parse_flexible_date).transpose()?,
        outcome: f.resultado.as_deref().map(str::parse::<EventOutcome>).transpose()?,
        action_prefix: f.accion,
        limit: Some(limite),
        ..Default::default()
    };

    let events = store.list_events(&filter)?;
    if events.is_empty() {
        println!("No hay eventos que coincidan con el filtro.");
        return Ok(());
    }

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
    if let Some(notas) = store.correcciones_diario(&[e.id])?.get(&e.id) {
        let lista: Vec<String> = notas.iter().map(|n| format!("#{n}")).collect();
        println!("  Corregido por: {}", lista.join(", "));
    }
    Ok(())
}

/// `ea diario anotar`: añade una nota del usuario; si corrige un evento, el
/// original queda intacto y la nota apunta a él.
pub fn anotar(store: &Store, nota: NuevaNota) -> Result<()> {
    let e = store.anotar_diario(nota)?;
    println!("Anotado en el diario: evento #{}.", e.id);
    Ok(())
}
