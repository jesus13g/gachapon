//! Abrir una terminal real del escritorio con un comando (art. IV.2: lo que
//! la web no puede confirmar, y las sesiones de la biblioteca, se abren en una
//! ventana del usuario). Es la lógica de lanzadores que comparten `ea-web`
//! (`⌨ abrir en la terminal`) y `ea_lanzar_sesion`.
//!
//! Sin shell: el comando va como argv al lanzador. Las pruebas usan
//! [`TerminalSimulada`]; nunca abren una ventana de verdad.

use std::sync::Mutex;

/// Los programas que abren una terminal con un comando, por orden de
/// preferencia: el estándar de freedesktop y el de Omarchy.
pub const LANZADORES: [&str; 2] = ["xdg-terminal-exec", "omarchy-launch-terminal"];

/// Quien sabe abrir una terminal con un comando.
pub trait Terminal: Send + Sync {
    /// Abre una terminal que ejecuta `argv` (sin shell) y vuelve sin
    /// esperar a que se cierre.
    fn abrir(&self, argv: &[String]) -> Result<(), String>;
}

/// Abre la terminal con el primer lanzador de [`LANZADORES`] que exista. Sin
/// esperar: la ventana vive lo que el usuario quiera; un hilo la recoge al
/// cerrarse para no dejar un proceso zombi.
pub fn lanzar_en_terminal(argv: &[String]) -> Result<(), String> {
    let mut ultimo = None;
    for lanzador in LANZADORES {
        let r = std::process::Command::new(lanzador)
            .args(argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match r {
            Ok(mut hijo) => {
                std::thread::spawn(move || hijo.wait());
                return Ok(());
            }
            Err(e) => ultimo = Some(format!("{lanzador}: {e}")),
        }
    }
    Err(format!("no hay terminal que abrir ({}); escribe el comando a mano", ultimo.unwrap_or_default()))
}

/// La terminal del sistema.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalSistema;

impl Terminal for TerminalSistema {
    fn abrir(&self, argv: &[String]) -> Result<(), String> {
        lanzar_en_terminal(argv)
    }
}

/// El doble de las pruebas: apunta cada argv que se habría abierto.
#[derive(Debug, Default)]
pub struct TerminalSimulada {
    pub lanzadas: Mutex<Vec<Vec<String>>>,
    /// Si es `Some`, abrir falla con este mensaje.
    pub fallo: Mutex<Option<String>>,
}

impl TerminalSimulada {
    pub fn lanzadas(&self) -> Vec<Vec<String>> {
        self.lanzadas.lock().unwrap().clone()
    }
}

impl Terminal for TerminalSimulada {
    fn abrir(&self, argv: &[String]) -> Result<(), String> {
        if let Some(m) = self.fallo.lock().unwrap().clone() {
            return Err(m);
        }
        self.lanzadas.lock().unwrap().push(argv.to_vec());
        Ok(())
    }
}
