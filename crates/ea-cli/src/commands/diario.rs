use anyhow::Result;
use comfy_table::Table;
use ea_store::{FiltroDiario, NuevaNota, Store};

use super::{fmt_ts, limpio};

/// `ea diario listar`, con los filtros tal como llegan de la línea de órdenes
/// (clap ya ha validado el resultado, el prefijo de la acción y el cursor).
pub fn listar(store: &Store, f: FiltroDiario) -> Result<()> {
    let filter = f.a_filtro(None)?;
    let events = store.list_events(&filter)?;
    if events.is_empty() {
        println!("No hay eventos que coincidan con el filtro.");
        return Ok(());
    }

    // Si la página va llena puede haber más: el cursor de la siguiente.
    let siguiente = filter.limit.is_some_and(|l| events.len() as i64 >= l).then(|| events.last().map(|e| e.id)).flatten();

    let mut table = Table::new();
    table.set_header(vec!["ID", "Cuándo", "Actor", "Acción", "Trabajo", "Resultado"]);
    for e in events {
        table.add_row(vec![
            e.id.to_string(),
            fmt_ts(e.ts),
            limpio(&e.actor),
            limpio(&e.action),
            limpio(&e.trabajo.unwrap_or_default()),
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
    // El actor, la acción y el trabajo de un evento de `ea-mcp` son los que
    // declaró o pidió el agente: sin secuencias de control en la terminal.
    println!("  Actor:     {}", limpio(&e.actor));
    println!("  Acción:    {}", limpio(&e.action));
    if let Some(trabajo) = &e.trabajo {
        println!("  Trabajo:   {}", limpio(trabajo));
    }
    if let Some(outcome) = e.outcome {
        println!("  Resultado: {}", outcome.as_str());
    }
    if let Some(duration) = e.duration_ms {
        println!("  Duración:  {duration} ms");
    }
    if let Some(detail) = &e.detail {
        println!("  Detalle:   {}", limpio(detail));
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
