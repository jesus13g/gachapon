//! `gachapon-instalar.exe` (spec 023, decisión 11): un único `.exe` para
//! Windows. Lleva dentro los binarios de gachapon, los extrae a una carpeta
//! temporal y ejecuta `ea.exe instalar` en **la misma consola**, para que la
//! confirmación se dé en una terminal real. Toda la lógica de instalación
//! está en `ea instalar`; esto solo la entrega.
//!
//! Es un binario de consola a propósito (sin `windows_subsystem`): con doble
//! clic Windows le abre una consola, y al final pide Intro para que no se
//! cierre antes de leer el resultado.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod binarios {
    include!(concat!(env!("OUT_DIR"), "/binarios.rs"));
}

/// Opción propia del instalador (no se pasa a `ea instalar`).
const SIN_PAUSA: &str = "--sin-pausa";

/// Los argumentos para `ea instalar` y si hay que pedir Intro al final.
fn separar_argumentos(args: &[String]) -> (Vec<String>, bool) {
    let pausa = !args.iter().any(|a| a == SIN_PAUSA);
    (args.iter().filter(|a| *a != SIN_PAUSA).cloned().collect(), pausa)
}

/// La carpeta temporal de esta ejecución.
fn carpeta_temporal(tmp: &Path, pid: u32) -> PathBuf {
    tmp.join(format!("gachapon-instalar-{pid}"))
}

/// Extrae los binarios a `dir` (la crea; sustituye lo que hubiera).
fn extraer(dir: &Path, binarios: &[(&str, &[u8])]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (nombre, bytes) in binarios {
        std::fs::write(dir.join(nombre), bytes)?;
    }
    Ok(())
}

fn pausar() {
    print!("\nPulsa Intro para cerrar.");
    let _ = std::io::stdout().flush();
    let mut linea = String::new();
    let _ = std::io::stdin().lock().read_line(&mut linea);
}

fn ejecutar(args: &[String]) -> Result<u8, String> {
    if binarios::BINARIOS.is_empty() {
        return Err("este gachapon-instalar.exe se compiló sin binarios (falta GACHAPON_BINARIOS al compilar); descarga el de la última Release".into());
    }
    let dir = carpeta_temporal(&std::env::temp_dir(), std::process::id());
    println!("gachapon: preparando la instalación en {}…", dir.display());
    extraer(&dir, binarios::BINARIOS).map_err(|e| format!("no se pudieron extraer los binarios en {}: {e}", dir.display()))?;
    let ea = dir.join("ea.exe");
    let estado = Command::new(&ea).arg("instalar").args(args).status();
    // Lo temporal se borra aunque `ea instalar` falle (si algo sigue en uso,
    // se queda: Windows limpia %TEMP% con el tiempo).
    let _ = std::fs::remove_dir_all(&dir);
    let estado = estado.map_err(|e| format!("no se pudo ejecutar {}: {e}", ea.display()))?;
    Ok(estado.code().map_or(1, |c| u8::try_from(c).unwrap_or(1)))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (para_ea, pausa) = separar_argumentos(&args);
    let codigo = match ejecutar(&para_ea) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gachapon-instalar: {e}");
            1
        }
    };
    if pausa {
        pausar();
    }
    ExitCode::from(codigo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_pausa_no_llega_a_ea_y_el_resto_si() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(separar_argumentos(&a(&[])), (vec![], true));
        assert_eq!(separar_argumentos(&a(&["--via", "wsl", "--sin-pausa"])), (a(&["--via", "wsl"]), false));
        assert_eq!(separar_argumentos(&a(&["--continuar"])), (a(&["--continuar"]), true));
    }

    #[test]
    fn la_carpeta_temporal_lleva_el_pid() {
        assert_eq!(carpeta_temporal(Path::new("/tmp"), 42), PathBuf::from("/tmp/gachapon-instalar-42"));
    }

    #[test]
    fn extrae_cada_binario_con_su_contenido() {
        let d = tempfile_dir();
        let destino = d.join("x");
        extraer(&destino, &[("ea.exe", b"uno"), ("ea-web.exe", b"dos")]).unwrap();
        assert_eq!(std::fs::read(destino.join("ea.exe")).unwrap(), b"uno");
        assert_eq!(std::fs::read(destino.join("ea-web.exe")).unwrap(), b"dos");
        // Repetir sustituye.
        extraer(&destino, &[("ea.exe", b"tres")]).unwrap();
        assert_eq!(std::fs::read(destino.join("ea.exe")).unwrap(), b"tres");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Carpeta temporal propia sin dependencias (el crate no tiene ninguna).
    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("gachapon-instalar-prueba-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
