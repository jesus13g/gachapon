use std::path::PathBuf;

use anyhow::Result;
use comfy_table::Table;
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
    let rutinas = store.listar_rutinas()?;
    if rutinas.is_empty() {
        println!("No hay rutinas registradas (ea rutina registrar <fichero.toml>).");
        return Ok(());
    }
    let mut table = Table::new();
    table.set_header(vec!["Rutina", "Versión", "Última ejecución", "Próxima (systemd)"]);
    for r in rutinas {
        let ultima = store
            .last_successful_run(&r.slug)?
            .map(|x| fmt_ts(x.started_at))
            .unwrap_or_else(|| "nunca".to_string());
        let proxima = proxima_ejecucion_systemd(&r.slug).unwrap_or_else(|| "—".to_string());
        table.add_row(vec![r.slug, r.version.to_string(), ultima, proxima]);
    }
    println!("{table}");
    Ok(())
}

pub fn ver(store: &Store, slug: &str) -> Result<()> {
    let fila = store
        .ultima_rutina(slug)?
        .ok_or_else(|| anyhow::anyhow!("la rutina '{slug}' no está registrada (ea rutina registrar <fichero>)"))?;
    println!("Rutina '{slug}', versión {} (registrada {} por {}):\n{}", fila.version, fila.creada_en, fila.creada_por, fila.definicion);
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

pub fn registrar(fichero: &std::path::Path) -> Result<()> {
    let texto = std::fs::read_to_string(fichero)
        .map_err(|e| anyhow::anyhow!("no se pudo leer {}: {e}", fichero.display()))?;
    let bus = super::trabajo::bus()?;
    let r = bus.registrar_rutina(&texto, &rutinas_dir())?;
    println!("rutina '{}' registrada en la versión {}", r.rutina.slug, r.rutina.version);
    Ok(())
}

/// Lo que invoca el `.service` plantilla de un disparador temporal: crea el
/// trabajo en el bus y vuelve; el orquestador lo ejecuta cuando puede.
pub fn lanzar(slug: &str, entradas: &[String]) -> Result<()> {
    let pares = entradas
        .iter()
        .map(|e| {
            e.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| anyhow::anyhow!("entrada inválida {e:?} (clave=valor)"))
        })
        .collect::<Result<Vec<_>>>()?;
    let id = super::trabajo::bus()?.lanzar_rutina(slug, &pares)?;
    println!("{id}");
    Ok(())
}
