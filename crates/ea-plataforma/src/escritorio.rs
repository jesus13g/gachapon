//! Integración con el escritorio: abrir, revelar, avisar y abrir terminales.
//! Nada pasa por un shell: los argumentos van como argv.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Lanza sin esperar y con la salida descartada; un hilo recoge al hijo para
/// no dejar zombis.
fn lanzar_y_soltar(mut cmd: Command) -> io::Result<()> {
    let mut hijo = cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    std::thread::spawn(move || {
        let _ = hijo.wait();
    });
    Ok(())
}

/// Abre un fichero, carpeta o URL con la aplicación predeterminada del
/// escritorio (`xdg-open`, `open`, `explorer`), sin esperar.
pub fn abrir(objetivo: &OsStr) -> io::Result<()> {
    let programa = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    let mut c = Command::new(programa);
    c.arg(objetivo);
    lanzar_y_soltar(c)
}

/// Muestra `ruta` seleccionada en el explorador de archivos. Linux:
/// `FileManager1.ShowItems` por D-Bus (`gdbus`) y, si nadie lo atiende,
/// `xdg-open` de su carpeta. macOS: `open -R`. Windows: `explorer /select,`.
pub fn revelar(ruta: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("open");
        c.arg("-R").arg(ruta);
        lanzar_y_soltar(c)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("explorer");
        c.raw_arg(format!("/select,\"{}\"", ruta.display()));
        lanzar_y_soltar(c)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let uri = uri_fichero(&ruta.display().to_string());
        let dbus = Command::new("gdbus")
            .args([
                "call", "--session", "--timeout", "5", "--dest", "org.freedesktop.FileManager1", "--object-path", "/org/freedesktop/FileManager1",
                "--method", "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("['{uri}']"))
            .arg("")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if dbus.is_ok_and(|st| st.success()) {
            return Ok(());
        }
        abrir(ruta.parent().unwrap_or(ruta).as_os_str())
    }
}

/// `file://` con todo lo que no es seguro en un URI (ni en la cadena GVariant
/// entre comillas simples que lo lleva) codificado en `%XX`.
pub fn uri_fichero(ruta: &str) -> String {
    let mut s = String::from("file://");
    for b in ruta.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(char::from(b));
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// Notificación de escritorio, mejor esfuerzo: Linux `notify-send`, macOS
/// `osascript`. En Windows devuelve `Err(Unsupported)`.
pub fn avisar(titulo: &str, cuerpo: &str) -> io::Result<()> {
    if cfg!(windows) {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "avisos de escritorio no soportados en Windows"));
    }
    let mut c = if cfg!(target_os = "macos") {
        let mut c = Command::new("osascript");
        c.arg("-e").arg(guion_aviso_macos(titulo, cuerpo));
        c
    } else {
        let mut c = Command::new("notify-send");
        c.arg("--").arg(titulo).arg(cuerpo);
        c
    };
    let st = c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    if st.success() { Ok(()) } else { Err(io::Error::other(format!("el aviso terminó con {st}"))) }
}

/// Literal de cadena de AppleScript: entre comillas, con `\` y `"` escapados.
fn literal_applescript(t: &str) -> String {
    let mut s = String::from("\"");
    for ch in t.chars() {
        match ch {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' | '\r' => s.push(' '),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Script de AppleScript de un aviso (puro, para probarlo).
pub fn guion_aviso_macos(titulo: &str, cuerpo: &str) -> String {
    format!("display notification {} with title {}", literal_applescript(cuerpo), literal_applescript(titulo))
}

/// Un comando que se abre en una terminal real del escritorio.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrdenTerminal {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub entorno: Vec<(String, String)>,
}

/// Los programas que se prueban, por orden, para abrir la terminal en este
/// sistema: en Linux el estándar de freedesktop y el de Omarchy; en macOS
/// `open` (Terminal.app); en Windows `cmd` (`start`).
pub fn lanzadores_terminal() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["open"]
    } else if cfg!(windows) {
        &["cmd"]
    } else {
        &["xdg-terminal-exec", "omarchy-launch-terminal"]
    }
}

/// Los argumentos que se pasan a `lanzador` (sin el propio programa). Es pura:
/// sirve para probar el argv sin lanzar nada.
///
/// - Linux: `env [-C cwd] K=V… argv…` (los lanzadores pueden pasar por systemd
///   y perder el cwd y el entorno).
/// - `open`: `-a Terminal`; la ruta del guion `.command` la añade
///   [`lanzar_en_terminal`].
/// - `cmd`: `/C start "" [/D cwd] argv…` (el entorno va en el `Command`).
pub fn argv_lanzador(orden: &OrdenTerminal, lanzador: &str) -> Vec<OsString> {
    let mut v: Vec<OsString> = Vec::new();
    match lanzador {
        "open" => v.extend(["-a", "Terminal"].map(OsString::from)),
        "cmd" => {
            v.extend(["/C", "start", ""].map(OsString::from));
            if let Some(c) = &orden.cwd {
                v.push("/D".into());
                v.push(c.as_os_str().to_owned());
            }
            v.extend(orden.argv.iter().map(OsString::from));
        }
        _ => {
            if orden.cwd.is_some() || !orden.entorno.is_empty() {
                v.push("env".into());
                if let Some(c) = &orden.cwd {
                    v.push("-C".into());
                    v.push(c.as_os_str().to_owned());
                }
                v.extend(orden.entorno.iter().map(|(k, x)| OsString::from(format!("{k}={x}"))));
            }
            v.extend(orden.argv.iter().map(OsString::from));
        }
    }
    v
}

/// Cita para sh con comillas simples.
fn citar_sh(t: &str) -> String {
    format!("'{}'", t.replace('\'', "'\\''"))
}

/// Contenido del `.command` de macOS: se borra a sí mismo, entra en el
/// directorio, exporta el entorno y hace `exec` del comando.
pub fn guion_command(orden: &OrdenTerminal) -> String {
    let mut s = String::from("#!/bin/sh\nrm -f -- \"$0\"\n");
    if let Some(c) = &orden.cwd {
        s.push_str(&format!("cd {} || exit 1\n", citar_sh(&c.display().to_string())));
    }
    for (k, v) in &orden.entorno {
        s.push_str(&format!("export {k}={}\n", citar_sh(v)));
    }
    s.push_str("exec");
    for a in &orden.argv {
        s.push(' ');
        s.push_str(&citar_sh(a));
    }
    s.push('\n');
    s
}

/// Abre una terminal que ejecuta la orden, sin esperar a que se cierre.
pub fn lanzar_en_terminal(orden: &OrdenTerminal) -> io::Result<()> {
    if orden.argv.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "orden de terminal vacía"));
    }
    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let ruta = std::env::temp_dir().join(format!("ea-terminal-{}-{nanos}.command", std::process::id()));
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o700).open(&ruta)?.write_all(guion_command(orden).as_bytes())?;
        let mut c = Command::new("open");
        c.args(argv_lanzador(orden, "open")).arg(&ruta);
        lanzar_y_soltar(c).inspect_err(|_| {
            let _ = std::fs::remove_file(&ruta);
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut ultimo = None;
        for lanzador in lanzadores_terminal() {
            let mut c = Command::new(lanzador);
            c.args(argv_lanzador(orden, lanzador));
            #[cfg(windows)]
            {
                c.envs(orden.entorno.iter().cloned());
            }
            match lanzar_y_soltar(c) {
                Ok(()) => return Ok(()),
                Err(e) => ultimo = Some(format!("{lanzador}: {e}")),
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, format!("no hay terminal que abrir ({})", ultimo.unwrap_or_default())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orden() -> OrdenTerminal {
        OrdenTerminal {
            argv: vec!["claude".into(), "--flag".into(), "a b".into()],
            cwd: Some(PathBuf::from("/tmp/mi dir")),
            entorno: vec![("EA_X".into(), "1".into()), ("Y".into(), "it's".into())],
        }
    }

    fn texto(v: Vec<OsString>) -> Vec<String> {
        v.into_iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn argv_de_linux_usa_env() {
        assert_eq!(
            texto(argv_lanzador(&orden(), "xdg-terminal-exec")),
            ["env", "-C", "/tmp/mi dir", "EA_X=1", "Y=it's", "claude", "--flag", "a b"]
        );
        let simple = OrdenTerminal { argv: vec!["ls".into()], ..Default::default() };
        assert_eq!(texto(argv_lanzador(&simple, "omarchy-launch-terminal")), ["ls"]);
    }

    #[test]
    fn argv_de_cmd_y_open() {
        assert_eq!(texto(argv_lanzador(&orden(), "cmd")), ["/C", "start", "", "/D", "/tmp/mi dir", "claude", "--flag", "a b"]);
        assert_eq!(texto(argv_lanzador(&orden(), "open")), ["-a", "Terminal"]);
    }

    #[test]
    fn guion_command_cita_con_comillas_simples() {
        let g = guion_command(&orden());
        assert!(g.starts_with("#!/bin/sh\nrm -f -- \"$0\"\n"));
        assert!(g.contains("cd '/tmp/mi dir' || exit 1\n"));
        assert!(g.contains("export EA_X='1'\n"));
        assert!(g.contains("export Y='it'\\''s'\n"));
        assert!(g.ends_with("exec 'claude' '--flag' 'a b'\n"));
    }

    #[test]
    fn aviso_de_macos_escapa() {
        assert_eq!(guion_aviso_macos("T \"x\"", "a\\b\nc"), "display notification \"a\\\\b c\" with title \"T \\\"x\\\"\"");
    }

    #[test]
    fn uri_codifica() {
        assert_eq!(uri_fichero("/a b/c'd"), "file:///a%20b/c%27d");
    }

    #[test]
    fn hay_lanzadores() {
        assert!(!lanzadores_terminal().is_empty());
        assert!(lanzar_en_terminal(&OrdenTerminal::default()).is_err());
    }
}
