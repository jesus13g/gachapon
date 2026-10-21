//! Órdenes externas de los comandos oficiales (`claude plugin enable`, etc.).
//!
//! Siempre por argv y sin shell. Un trait para que las pruebas usen un doble
//! que apunta el argv exacto en vez de lanzar nada.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::ErrorBiblioteca;

/// Tope de una orden oficial.
pub const LIMITE: Duration = Duration::from_secs(30);
/// Cuánta salida se conserva de cada flujo.
const MAX_SALIDA: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SalidaOrden {
    /// `None`: la mató una señal.
    pub codigo: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl SalidaOrden {
    pub fn exito(&self) -> bool {
        self.codigo == Some(0)
    }
}

pub trait Ordenes: Send + Sync {
    /// ¿Se puede lanzar `bin` (nombre del binario, sin ruta)?
    fn disponible(&self, bin: &str) -> bool;

    /// Lanza `argv` (el primero es el binario) en `cwd`, sin shell y con tope
    /// de tiempo. `Err` solo si no se pudo lanzar o se pasó del tiempo; un
    /// código de salida distinto de cero es una `SalidaOrden` normal.
    fn ejecutar(&self, argv: &[String], cwd: Option<&Path>) -> Result<SalidaOrden, ErrorBiblioteca>;
}

/// Las órdenes de verdad: el binario se busca en el `PATH` con
/// `ea_ajustes::buscar_binario`.
#[derive(Debug, Clone, Default)]
pub struct OrdenesReales;

fn leer_acotado(mut r: impl Read) -> String {
    let mut buf = Vec::new();
    let mut trozo = [0u8; 4096];
    while let Ok(n) = r.read(&mut trozo) {
        if n == 0 {
            break;
        }
        // Se sigue leyendo (para no bloquear al proceso) pero solo se guarda el principio.
        if buf.len() < MAX_SALIDA {
            buf.extend_from_slice(&trozo[..n.min(MAX_SALIDA - buf.len())]);
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

impl Ordenes for OrdenesReales {
    fn disponible(&self, bin: &str) -> bool {
        ea_ajustes::buscar_binario(bin, std::env::var_os("PATH").as_deref()).is_some()
    }

    fn ejecutar(&self, argv: &[String], cwd: Option<&Path>) -> Result<SalidaOrden, ErrorBiblioteca> {
        let (bin, args) = argv.split_first().ok_or_else(|| ErrorBiblioteca::Orden("orden vacía".into()))?;
        let ruta: PathBuf = ea_ajustes::buscar_binario(bin, std::env::var_os("PATH").as_deref())
            .ok_or_else(|| ErrorBiblioteca::BinarioNoDisponible(bin.clone()))?;
        let mut cmd = Command::new(ruta);
        cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        let mut hijo = cmd.spawn().map_err(|e| ErrorBiblioteca::Orden(format!("no se pudo lanzar {bin}: {e}")))?;
        let salida = hijo.stdout.take().map(|s| std::thread::spawn(move || leer_acotado(s)));
        let error = hijo.stderr.take().map(|s| std::thread::spawn(move || leer_acotado(s)));
        let inicio = Instant::now();
        let estado = loop {
            match hijo.try_wait() {
                Ok(Some(e)) => break e,
                Ok(None) if inicio.elapsed() >= LIMITE => {
                    let _ = hijo.kill();
                    let _ = hijo.wait();
                    return Err(ErrorBiblioteca::Orden(format!("{bin} tardó más de {} s", LIMITE.as_secs())));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(25)),
                Err(e) => return Err(ErrorBiblioteca::Orden(format!("{bin}: {e}"))),
            }
        };
        Ok(SalidaOrden {
            codigo: estado.code(),
            stdout: salida.and_then(|h| h.join().ok()).unwrap_or_default(),
            stderr: error.and_then(|h| h.join().ok()).unwrap_or_default(),
        })
    }
}

/// Un doble para pruebas: apunta cada argv y responde con la salida fijada.
#[derive(Debug)]
pub struct OrdenesDoble {
    /// Binarios que «existen»; `None`: todos.
    pub binarios: Option<Vec<String>>,
    /// Qué responder; por defecto, éxito sin salida.
    pub respuesta: Mutex<SalidaOrden>,
    /// Cada llamada: argv y directorio de trabajo.
    pub llamadas: Mutex<Vec<(Vec<String>, Option<PathBuf>)>>,
}

impl Default for OrdenesDoble {
    fn default() -> Self {
        Self { binarios: None, respuesta: Mutex::new(SalidaOrden { codigo: Some(0), ..Default::default() }), llamadas: Mutex::new(Vec::new()) }
    }
}

impl OrdenesDoble {
    pub fn argvs(&self) -> Vec<Vec<String>> {
        self.llamadas.lock().unwrap().iter().map(|(a, _)| a.clone()).collect()
    }

    pub fn fallando(self, codigo: i32, stderr: &str) -> Self {
        *self.respuesta.lock().unwrap() = SalidaOrden { codigo: Some(codigo), stdout: String::new(), stderr: stderr.into() };
        self
    }

    pub fn solo_con(mut self, binarios: &[&str]) -> Self {
        self.binarios = Some(binarios.iter().map(|b| b.to_string()).collect());
        self
    }
}

impl Ordenes for OrdenesDoble {
    fn disponible(&self, bin: &str) -> bool {
        self.binarios.as_ref().is_none_or(|l| l.iter().any(|b| b == bin))
    }

    fn ejecutar(&self, argv: &[String], cwd: Option<&Path>) -> Result<SalidaOrden, ErrorBiblioteca> {
        let bin = argv.first().ok_or_else(|| ErrorBiblioteca::Orden("orden vacía".into()))?;
        if !self.disponible(bin) {
            return Err(ErrorBiblioteca::BinarioNoDisponible(bin.clone()));
        }
        self.llamadas.lock().unwrap().push((argv.to_vec(), cwd.map(Path::to_path_buf)));
        Ok(self.respuesta.lock().unwrap().clone())
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn las_reales_lanzan_sin_shell_y_recogen_la_salida() {
        // Con shell, «$HOME» se expandiría y «;» separaría órdenes.
        let s = OrdenesReales.ejecutar(&argv(&["echo", "$HOME;", "x"]), None).unwrap();
        assert!(s.exito());
        assert_eq!(s.stdout.trim(), "$HOME; x");
        let s = OrdenesReales.ejecutar(&argv(&["false"]), None).unwrap();
        assert_eq!(s.codigo, Some(1));
        assert!(matches!(OrdenesReales.ejecutar(&argv(&["ea-no-existe-nunca"]), None), Err(ErrorBiblioteca::BinarioNoDisponible(_))));
        assert!(!OrdenesReales.disponible("ea-no-existe-nunca"));
    }
}
