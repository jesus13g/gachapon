pub mod acciones;
pub mod agentes;
pub mod ajustes;
pub mod biblioteca;
pub mod boveda;
pub mod diario;
pub mod equipo;
pub mod hook;
pub mod instalar;
pub mod propuestas;
pub mod qwen_skills;
pub mod rutinas;
pub mod servicios;
pub mod status;
pub mod trabajo;
pub mod worktree;

use time::macros::format_description;
use time::OffsetDateTime;

/// Texto de origen externo (lo que escriben los agentes, la salida de un
/// criterio, un fichero ajeno) listo para la terminal: sin caracteres de
/// control salvo `\n` y `\t`, así que tampoco secuencias ANSI u OSC (un OSC 52
/// escribe en el portapapeles). Solo en la salida de tabla: `--json` ya los
/// escapa.
pub fn limpio(texto: &str) -> String {
    ea_biblioteca::sin_control(texto)
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
    ea_acciones::SistemaReal::usuario(ea_ajustes::cargar().orquestador.rutinas_dir, ea_boveda::Boveda::configurada(), None)
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
    println!("  Simulación:  {}", limpio(simulacion));
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
    use super::{limpio, respuesta_confirma};

    #[test]
    fn lo_que_se_imprime_no_lleva_secuencias_de_escape() {
        assert_eq!(limpio("\x1b[31mrojo\x1b[0m\x07 ok\n"), "[31mrojo[0m ok\n");
        assert_eq!(limpio("a\x1b]52;c;eA==\x07b\u{9b}2J\r\tc"), "a]52;c;eA==b2J\tc");
    }

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
