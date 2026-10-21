pub mod acciones;
pub mod agentes;
pub mod ajustes;
pub mod biblioteca;
pub mod boveda;
pub mod diario;
pub mod propuestas;
pub mod qwen_skills;
pub mod rutinas;
pub mod status;
pub mod trabajo;

use anyhow::{Context, Result};
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::OffsetDateTime;

/// Acepta tanto una fecha-hora completa RFC3339 como una fecha simple
/// AAAA-MM-DD (interpretada como medianoche UTC), para que los filtros
/// `--desde`/`--hasta` sean cómodos de escribir a mano.
pub fn parse_flexible_date(s: &str) -> Result<OffsetDateTime> {
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Ok(dt);
    }
    let date_fmt = format_description!("[year]-[month]-[day]");
    let date = time::Date::parse(s, &date_fmt)
        .with_context(|| format!("fecha inválida: '{s}' (usa RFC3339 o AAAA-MM-DD)"))?;
    Ok(date.midnight().assume_utc())
}

/// Formatea un timestamp para mostrarlo en tablas: legible, en UTC.
pub fn fmt_ts(dt: OffsetDateTime) -> String {
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");
    dt.format(&fmt).unwrap_or_else(|_| dt.to_string())
}

/// Ejecuta un futuro de la capa de acciones desde el `main` síncrono.
pub fn bloquear<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime de tokio")
        .block_on(f)
}

/// El sistema real para lo que el usuario confirma desde esta terminal.
pub fn sistema() -> ea_acciones::SistemaReal {
    ea_acciones::SistemaReal {
        rutinas_dir: ea_ajustes::cargar().orquestador.rutinas_dir,
        boveda: ea_acciones::Boveda::configurada(),
        propio: None,
        temporizadores: ea_acciones::Temporizadores::usuario(),
    }
}

/// Pregunta en la terminal y devuelve la línea escrita.
pub fn preguntar(texto: &str) -> anyhow::Result<String> {
    use std::io::Write;
    print!("{texto}");
    std::io::stdout().flush()?;
    let mut linea = String::new();
    std::io::stdin().read_line(&mut linea)?;
    Ok(linea.trim().to_string())
}

/// Con `esperar` (una ventana abierta por ea-web), enseña el error, si lo
/// hay, y espera a Intro antes de salir: si no, la ventana se cerraría sin
/// dejar leer el resultado. El error se devuelve igual (código de salida).
pub fn al_final(esperar: bool, r: anyhow::Result<()>) -> anyhow::Result<()> {
    if esperar {
        if let Err(e) = &r {
            eprintln!("Error: {e:#}");
        }
        let _ = preguntar("\n  Pulsa Intro para cerrar esta ventana. ");
    }
    r
}

/// Confirmación explícita (§11.1): una reversible pide `s`; una irreversible
/// obliga a escribir `confirmo <referencia>`, para que no se acepte por
/// reflejo ni con un simple Enter.
pub fn respuesta_confirma(irreversible: bool, referencia: &str, respuesta: &str) -> bool {
    let r = respuesta.trim().to_lowercase();
    if irreversible {
        r == format!("confirmo {}", referencia.to_lowercase())
    } else {
        matches!(r.as_str(), "s" | "si" | "sí" | "y" | "yes")
    }
}

/// Muestra la simulación y exige confirmación en una terminal interactiva.
/// Devuelve `Ok(true)` solo si el usuario confirmó.
pub fn confirmar_en_terminal(
    herramienta: &str,
    riesgo: &str,
    argumentos: &serde_json::Value,
    simulacion: &str,
    referencia: &str,
) -> anyhow::Result<bool> {
    let irreversible = riesgo == "irreversible";
    println!("  Herramienta: {herramienta}   Riesgo: {}", riesgo.to_uppercase());
    println!("  Argumentos:  {argumentos}");
    println!("  Simulación:  {simulacion}");
    let respuesta = if irreversible {
        println!("\n  ACCIÓN IRREVERSIBLE: no hay vuelta atrás.");
        preguntar(&format!("  Para ejecutarla escribe «confirmo {referencia}» (otra cosa cancela): "))?
    } else {
        preguntar("  ¿Ejecutar ahora? [s/N]: ")?
    };
    Ok(respuesta_confirma(irreversible, referencia, &respuesta))
}

#[cfg(test)]
mod tests {
    use super::respuesta_confirma;

    #[test]
    fn la_confirmacion_es_explicita() {
        assert!(respuesta_confirma(false, "3", "s"));
        assert!(respuesta_confirma(false, "3", " Sí "));
        assert!(!respuesta_confirma(false, "3", ""));
        assert!(!respuesta_confirma(false, "3", "n"));
        // Una irreversible no se acepta con un simple «s»: hay que escribir la frase.
        assert!(!respuesta_confirma(true, "3", "s"));
        assert!(!respuesta_confirma(true, "3", ""));
        assert!(!respuesta_confirma(true, "3", "confirmo 4"));
        assert!(respuesta_confirma(true, "3", "confirmo 3"));
    }
}
