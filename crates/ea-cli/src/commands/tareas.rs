use anyhow::Result;
use comfy_table::Table;
use ea_store::{NewTask, Store, TaskFilter, TaskOrigin, TaskStatus};

use super::fmt_ts;

pub fn crear(
    store: &mut Store,
    titulo: String,
    asignado_a: String,
    solicita: String,
    origen: TaskOrigin,
    creado_por: String,
    tarea_padre: Option<i64>,
) -> Result<()> {
    let task = store.create_task(NewTask {
        created_by: creado_por,
        assigned_to: asignado_a,
        origin: origen,
        parent_task_id: tarea_padre,
        title: titulo,
        request: solicita,
    })?;
    println!("Tarea #{} creada", task.id);
    Ok(())
}

pub fn listar(
    store: &Store,
    estado: Option<TaskStatus>,
    asignado_a: Option<String>,
    limite: i64,
) -> Result<()> {
    let tasks = store.list_tasks(&TaskFilter {
        status: estado,
        assigned_to: asignado_a,
        limit: Some(limite),
    })?;

    if tasks.is_empty() {
        println!("No hay tareas que coincidan con el filtro.");
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec![
        "ID", "Título", "Asignada a", "Estado", "Origen", "Creada",
    ]);
    for t in tasks {
        table.add_row(vec![
            t.id.to_string(),
            t.title,
            t.assigned_to,
            t.status.as_str().to_string(),
            t.origin.as_str().to_string(),
            fmt_ts(t.created_at),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn ver(store: &Store, id: i64) -> Result<()> {
    let task = store.get_task(id)?;

    println!("Tarea #{}: {}", task.id, task.title);
    println!("  Creada por:   {}", task.created_by);
    println!("  Asignada a:   {}", task.assigned_to);
    println!("  Origen:       {}", task.origin.as_str());
    println!("  Estado:       {}", task.status.as_str());
    println!("  Creada:       {}", fmt_ts(task.created_at));
    println!("  Actualizada:  {}", fmt_ts(task.updated_at));
    if let Some(parent) = task.parent_task_id {
        println!("  Tarea padre:  #{parent}");
    }
    println!("  Solicita:     {}", task.request);
    if let Some(result) = &task.result {
        println!("  Resultado:    {result}");
    }

    let mut events = store.list_events_by_task(id)?;
    events.reverse(); // orden cronológico ascendente: la historia de la tarea

    if !events.is_empty() {
        println!("\nHistorial de eventos:");
        let mut table = Table::new();
        table.set_header(vec!["Cuándo", "Actor", "Acción", "Resultado", "Duración"]);
        for e in events {
            table.add_row(vec![
                fmt_ts(e.ts),
                e.actor,
                e.action,
                e.outcome.map(|o| o.as_str().to_string()).unwrap_or_default(),
                e.duration_ms
                    .map(|d| format!("{d} ms"))
                    .unwrap_or_default(),
            ]);
        }
        println!("{table}");
    }

    Ok(())
}

pub fn actualizar(
    store: &mut Store,
    id: i64,
    estado: TaskStatus,
    resultado: Option<String>,
    actor: String,
) -> Result<()> {
    let task = store.update_task_status(id, estado, &actor, resultado)?;
    println!("Tarea #{} actualizada a '{}'", task.id, task.status.as_str());
    Ok(())
}
