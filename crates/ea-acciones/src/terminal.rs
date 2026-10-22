//! Abrir una terminal real del escritorio con un comando (art. IV.2: lo que
//! la web no puede confirmar, y las sesiones de la biblioteca, se abren en una
//! ventana del usuario). Es la lógica que comparten `ea-web` (`⌨ abrir en la
//! terminal`), `ea equipo despertar` y `ea_lanzar_sesion`.
//!
//! El lanzador de cada sistema (xdg-terminal-exec y Omarchy en Linux,
//! Terminal.app en macOS, `cmd start` en Windows) vive en
//! `ea_plataforma::escritorio`: aquí solo se construye la [`OrdenTerminal`].
//! Sin shell: el comando va como argv. Las pruebas usan [`TerminalSimulada`];
//! nunca abren una ventana de verdad.

use std::sync::Mutex;

pub use ea_plataforma::escritorio::OrdenTerminal;
use ea_plataforma::escritorio::{argv_lanzador, lanzar_en_terminal as lanzar_orden_del_sistema};

/// Quien sabe abrir una terminal con un comando.
pub trait Terminal: Send + Sync {
    /// Abre una terminal que ejecuta `argv` (sin shell) y vuelve sin
    /// esperar a que se cierre.
    fn abrir(&self, argv: &[String]) -> Result<(), String>;

    /// Lo mismo con directorio y entorno. Por defecto lo reduce al argv de
    /// Linux (`env -C <cwd> K=V… argv`, [`argv_linux`]); la terminal del
    /// sistema lo traduce a su lanzador.
    fn abrir_orden(&self, orden: &OrdenTerminal) -> Result<(), String> {
        self.abrir(&argv_linux(orden))
    }
}

/// El argv de Linux de una orden: `env [-C cwd] K=V… argv…`, o el argv a
/// secas si no hay directorio ni entorno. Es puro.
pub fn argv_linux(orden: &OrdenTerminal) -> Vec<String> {
    argv_lanzador(orden, "xdg-terminal-exec").into_iter().map(|a| a.to_string_lossy().into_owned()).collect()
}

/// Abre la terminal con la orden en el lanzador del sistema, sin esperar: la
/// ventana vive lo que el usuario quiera.
pub fn lanzar_orden(orden: &OrdenTerminal) -> Result<(), String> {
    lanzar_orden_del_sistema(orden).map_err(|e| format!("{e}; escribe el comando a mano"))
}

/// Abre la terminal con `argv` (sin directorio ni entorno propios).
pub fn lanzar_en_terminal(argv: &[String]) -> Result<(), String> {
    lanzar_orden(&OrdenTerminal { argv: argv.to_vec(), ..Default::default() })
}

/// La terminal del sistema.
#[derive(Debug, Clone, Copy, Default)]
pub struct TerminalSistema;

impl Terminal for TerminalSistema {
    fn abrir(&self, argv: &[String]) -> Result<(), String> {
        lanzar_en_terminal(argv)
    }

    fn abrir_orden(&self, orden: &OrdenTerminal) -> Result<(), String> {
        lanzar_orden(orden)
    }
}

/// El doble de las pruebas: apunta cada argv que se habría abierto (en la
/// forma de Linux, con `env -C` si la orden lleva directorio o entorno).
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

#[cfg(test)]
mod pruebas {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn el_argv_de_linux_es_el_de_siempre() {
        let o = OrdenTerminal {
            argv: vec!["claude".into(), "--x".into()],
            cwd: Some(PathBuf::from("/p")),
            entorno: vec![("A".into(), "1".into())],
        };
        assert_eq!(argv_linux(&o), ["env", "-C", "/p", "A=1", "claude", "--x"]);
        let simple = OrdenTerminal { argv: vec!["ea".into(), "x".into()], ..Default::default() };
        assert_eq!(argv_linux(&simple), ["ea", "x"]);
    }

    #[test]
    fn la_simulada_recibe_la_orden_como_argv_de_linux() {
        let t = TerminalSimulada::default();
        let o = OrdenTerminal { argv: vec!["claude".into()], cwd: Some(PathBuf::from("/p")), entorno: vec![] };
        t.abrir_orden(&o).unwrap();
        assert_eq!(t.lanzadas(), [["env", "-C", "/p", "claude"]]);
    }
}
