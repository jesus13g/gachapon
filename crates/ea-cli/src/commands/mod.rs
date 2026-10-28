pub mod acciones;
pub mod agentes;
pub mod ajustes;
pub mod biblioteca;
pub mod boveda;
pub mod diario;
pub mod equipo;
pub mod hook;
pub mod instalar;
pub mod mcp;
pub mod propuestas;
pub mod qwen_skills;
pub mod rutinas;
pub mod servicios;
pub mod status;
pub mod trabajo;
pub mod worktree;

use anyhow::Result;
use ea_acciones::{SistemaReal, SoloConsulta, Temporizadores};
use ea_ajustes::Ajustes;
use ea_bus::Bus;
use ea_store::{AccionPropuesta, Propuesta, Store};
use time::macros::format_description;
use time::OffsetDateTime;

/// Quién firma en el diario lo que esta CLI hace en el bus (U-06): el usuario
/// si stdin y stdout son una terminal interactiva; sin ella, quien lanza `ea`
/// es un agente con Bash, el planificador, con el mismo actor que le da el MCP
/// (su rol en disco). Los eventos viejos siguen firmados como `ea-cli`.
pub fn actor() -> &'static str {
    actor_segun(ea_acciones::terminal_interactiva().is_ok())
}

fn actor_segun(con_terminal: bool) -> &'static str {
    if con_terminal { ea_ajustes::ACTOR_USUARIO } else { ea_ajustes::Rol::Planificador.as_str() }
}

/// Lo que comparten las órdenes de un `ea`: los ajustes, cargados una vez, y
/// la base de datos y los timers de las rutinas, que se abren la primera vez
/// que una orden los pide (una que no los usa no los abre).
#[derive(Default)]
pub struct Contexto {
    ajustes: Option<Ajustes>,
    store: Option<Store>,
    temporizadores: Option<Temporizadores>,
}

impl Contexto {
    pub fn ajustes(&mut self) -> &Ajustes {
        self.ajustes.get_or_insert_with(ea_ajustes::cargar)
    }

    /// La base de datos (`rutas.db`).
    pub fn store(&mut self) -> Result<&mut Store> {
        if self.store.is_none() {
            self.store = Some(Store::open_at(&self.ajustes().rutas.db)?);
        }
        Ok(self.store.as_mut().expect("recién abierta"))
    }

    /// Los timers de las rutinas de este sistema ([`Temporizadores::usuario`]).
    pub fn temporizadores(&mut self) -> &Temporizadores {
        self.temporizadores.get_or_insert_with(Temporizadores::usuario)
    }

    /// El bus con el historial en la base, que pasa al bus, firmado con
    /// [`actor`]: para lo que crea o mueve trabajos y rutinas.
    pub fn bus(&mut self) -> Result<Bus> {
        let store = match self.store.take() {
            Some(s) => s,
            None => Store::open_at(&self.ajustes().rutas.db)?,
        };
        Ok(Bus::abrir_en(&self.ajustes().rutas.estado)?.con_store(store, actor()))
    }

    /// El sistema real para lo que el usuario confirma desde esta terminal.
    pub fn sistema(&mut self) -> SistemaReal {
        let temporizadores = self.temporizadores().clone();
        let a = self.ajustes();
        SistemaReal {
            consulta: SoloConsulta {
                rutinas_dir: a.orquestador.rutinas_dir.clone(),
                boveda: ea_boveda::Boveda::en(a.rutas.boveda.clone()),
                estado_dir: a.rutas.estado.clone(),
                temporizadores,
                biblioteca: None,
            },
            propio: None,
            terminal: std::sync::Arc::new(ea_acciones::TerminalSistema),
        }
    }
}

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

/// Pregunta en la terminal y devuelve la línea escrita.
pub fn preguntar(texto: &str) -> Result<String> {
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
pub fn al_final(esperar: bool, r: Result<()>) -> Result<()> {
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

/// Con la acción de la propuesta `id` a la vista, pide su confirmación en
/// esta terminal (una irreversible exige escribir `confirmo <id>`) y solo
/// entonces la acepta como el usuario, que la ejecuta. `None`: no se confirmó
/// y no se ha hecho nada. Quien llama ya ha comprobado la terminal (`canal`).
pub fn confirmar_y_aceptar(store: &mut Store, sis: &SistemaReal, id: i64, a: &AccionPropuesta, canal: ea_acciones::Canal) -> Result<Option<Propuesta>> {
    if !confirmar_en_terminal(&a.herramienta, &a.riesgo, &a.argumentos, &a.simulacion, &id.to_string())? {
        return Ok(None);
    }
    Ok(Some(bloquear(ea_acciones::aceptar_propuesta(store, sis, id, ea_ajustes::ACTOR_USUARIO, canal))?))
}

/// Muestra la simulación y exige confirmación en una terminal interactiva.
/// Devuelve `Ok(true)` solo si el usuario confirmó.
pub fn confirmar_en_terminal(
    herramienta: &str,
    riesgo: &str,
    argumentos: &serde_json::Value,
    simulacion: &str,
    referencia: &str,
) -> Result<bool> {
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
    use super::{actor_segun, limpio, respuesta_confirma};

    #[test]
    fn la_cli_firma_como_usuario_con_terminal_y_como_planificador_sin_ella() {
        assert_eq!(actor_segun(true), ea_ajustes::ACTOR_USUARIO);
        // El mismo actor que el MCP da al planificador con credencial.
        assert_eq!(actor_segun(false), ea_ajustes::Rol::Planificador.as_str());
    }

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
