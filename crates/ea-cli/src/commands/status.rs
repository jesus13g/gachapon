use anyhow::Result;
use comfy_table::Table;
use ea_store::{EstadoPropuesta, EventFilter, PropuestaFilter, Store, TaskFilter, TaskStatus};

use super::fmt_ts;

/// Salud del orquestador derivada del heartbeat más reciente en el diario
/// (§13.2, y la nota dejada en fase 1: "cuando el orquestador real exista,
/// la forma correcta de liveness es que él mismo emita eventos
/// actor='orquestador', action='heartbeat'"). Nunca inventa un heartbeat
/// contra un proceso que no responde: si no hay ninguno, dice que no hay
/// señal, no que esté "caído".
pub fn estado_orquestador(store: &Store) -> Result<String> {
    let umbral_s = (ea_orchestrator::config::load().intervalo_heartbeat_s * 2) as i64;
    let estado = store.estado_orquestador(umbral_s)?;
    Ok(match estado.ultimo_latido {
        None => "sin señal (ningún heartbeat registrado todavía)".to_string(),
        Some(t) if estado.vivo => format!("vivo (heartbeat {})", fmt_ts(t)),
        Some(t) => format!("sin señal reciente (último heartbeat {})", fmt_ts(t)),
    })
}

const ALL_STATUSES: [TaskStatus; 5] = [
    TaskStatus::Pendiente,
    TaskStatus::EnProgreso,
    TaskStatus::Completada,
    TaskStatus::Fallida,
    TaskStatus::Cancelada,
];

/// Resumen no interactivo del estado del sistema: una sola invocación,
/// apta para mostrarse al iniciar sesión (§12.4 de la especificación).
pub fn mostrar(store: &Store) -> Result<()> {
    println!("Ecosistema de asistente — estado");
    println!("---------------------------------");
    println!("Orquestador: {}", estado_orquestador(store)?);
    let pendientes = store
        .list_propuestas(&PropuestaFilter { estado: Some(EstadoPropuesta::Pendiente), limit: None })?
        .len();
    println!("Propuestas pendientes: {pendientes}");
    println!("Base de datos: {}", ea_store::resolve_db_path()?.display());
    println!();

    println!("Tareas por estado:");
    let mut table = Table::new();
    table.set_header(vec!["Estado", "Cantidad"]);
    for status in ALL_STATUSES {
        let count = store
            .list_tasks(&TaskFilter {
                status: Some(status),
                ..Default::default()
            })?
            .len();
        table.add_row(vec![status.as_str().to_string(), count.to_string()]);
    }
    println!("{table}");

    println!("\nÚltimos eventos:");
    let events = store.list_events(&EventFilter {
        limit: Some(5),
        ..Default::default()
    })?;
    if events.is_empty() {
        println!("  (sin eventos todavía)");
    } else {
        let mut table = Table::new();
        table.set_header(vec!["Cuándo", "Actor", "Acción", "Tarea"]);
        for e in events {
            table.add_row(vec![
                fmt_ts(e.ts),
                e.actor,
                e.action,
                e.task_id.map(|t| format!("#{t}")).unwrap_or_default(),
            ]);
        }
        println!("{table}");
    }

    Ok(())
}
