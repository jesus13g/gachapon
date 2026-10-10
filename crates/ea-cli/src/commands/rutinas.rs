use std::path::PathBuf;

use anyhow::Result;
use comfy_table::Table;
use ea_orchestrator::rutina;
use ea_store::Store;

use super::fmt_ts;

fn rutinas_dir() -> PathBuf {
    ea_orchestrator::config::load().rutinas_dir
}

/// Próxima ejecución programada según `systemd --user`, si el timer de la
/// rutina está instalado. Mejor esfuerzo: la CLI no habla con ningún
/// agente (§12.2), pero preguntarle al gestor de servicios del sistema por
/// su propio estado no lo es — y si `systemctl` no está disponible o el
/// timer no está instalado, se omite sin error (§12.4: degradación limpia).
fn proxima_ejecucion_systemd(slug: &str) -> Option<String> {
    let salida = std::process::Command::new("systemctl")
        .args(["--user", "list-timers", &format!("ea-rutina@{slug}.timer"), "--no-legend"])
        .output()
        .ok()?;
    if !salida.status.success() {
        return None;
    }
    let linea = String::from_utf8_lossy(&salida.stdout);
    let linea = linea.lines().next()?.trim();
    if linea.is_empty() {
        None
    } else {
        Some(linea.to_string())
    }
}

pub fn listar(store: &Store) -> Result<()> {
    let dir = rutinas_dir();
    let slugs = rutina::listar_slugs(&dir)?;

    if slugs.is_empty() {
        println!(
            "No hay rutinas en {} (copia un fichero .toml ahí para darla de alta).",
            dir.display()
        );
        return Ok(());
    }

    let mut table = Table::new();
    table.set_header(vec!["Rutina", "Descripción", "Última ejecución", "Próxima (systemd)"]);
    for slug in slugs {
        let descripcion = match rutina::cargar(&dir, &slug) {
            Ok(r) => r.descripcion,
            Err(e) => format!("(inválida: {e})"),
        };
        let ultima = store
            .last_successful_run(&slug)?
            .map(|r| fmt_ts(r.started_at))
            .unwrap_or_else(|| "nunca".to_string());
        let proxima = proxima_ejecucion_systemd(&slug).unwrap_or_else(|| "—".to_string());
        table.add_row(vec![slug, descripcion, ultima, proxima]);
    }
    println!("{table}");
    Ok(())
}

pub fn ver(store: &Store, slug: &str) -> Result<()> {
    let dir = rutinas_dir();
    let rutina = rutina::cargar(&dir, slug)?;

    println!("Rutina '{slug}': {}", rutina.descripcion);
    println!("  Inicio:                {}", rutina.inicio);
    println!("  Presupuesto de tiempo:  {} s", rutina.max_duracion_s);
    println!("  Presupuesto de pasos:   {}", rutina.max_pasos);
    println!("  Presupuesto de modelo:  {} llamadas", rutina.max_llamadas_modelo);
    println!("  Ventana inicial:        {} día(s) (si nunca se ha ejecutado)", rutina.ventana_inicial_dias);
    if let Some(proxima) = proxima_ejecucion_systemd(slug) {
        println!("  Próxima (systemd):      {proxima}");
    }

    let historial = store.list_routine_runs(slug, Some(20))?;
    if historial.is_empty() {
        println!("\nSin ejecuciones todavía.");
        return Ok(());
    }

    println!("\nHistorial de ejecuciones:");
    let mut table = Table::new();
    table.set_header(vec!["Cuándo", "Resultado", "Pasos", "Llamadas modelo", "Detalle"]);
    for run in historial {
        table.add_row(vec![
            fmt_ts(run.started_at),
            run.outcome.map(|o| o.as_str().to_string()).unwrap_or_else(|| "en curso".to_string()),
            run.pasos_ejecutados.to_string(),
            run.llamadas_modelo.to_string(),
            run.detalle.unwrap_or_default(),
        ]);
    }
    println!("{table}");
    Ok(())
}

/// El único comando que invoca el `.service` plantilla de un disparador
/// temporal (§10.1: el disparador no ejecuta la rutina, solo encola una
/// tarea que el orquestador recoge cuando puede).
pub fn encolar(store: &mut Store, slug: &str) -> Result<()> {
    // Falla pronto si la rutina no existe o es inválida, en vez de encolar
    // una tarea que el orquestador tendría que rechazar de todas formas.
    let dir = rutinas_dir();
    rutina::cargar(&dir, slug)?;

    let tarea = store.create_task(ea_store::NewTask {
        created_by: "ea-rutina-timer".to_string(),
        assigned_to: "orquestador".to_string(),
        origin: ea_store::TaskOrigin::Programado,
        parent_task_id: None,
        title: format!("Ejecutar rutina: {slug}"),
        request: serde_json::json!({ "rutina": slug }).to_string(),
    })?;
    println!("Tarea #{} encolada para la rutina '{slug}'", tarea.id);
    Ok(())
}
