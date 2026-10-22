//! Rutas y búsqueda de programas.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Separador de las entradas de `PATH`.
#[cfg(windows)]
pub const SEPARADOR_PATH: char = ';';
/// Separador de las entradas de `PATH`.
#[cfg(not(windows))]
pub const SEPARADOR_PATH: char = ':';

/// Nombre del ejecutable de `base` en este sistema (`ea` -> `ea.exe` en Windows).
pub fn nombre_exe(base: &str) -> String {
    if cfg!(windows) { format!("{base}.exe") } else { base.to_string() }
}

/// Directorio de ejecución del usuario (efímero, privado), donde vive p. ej.
/// la clave de sesión del panel. No crea nada. `var` lee variables de entorno.
///
/// - Linux: `XDG_RUNTIME_DIR` si es absoluto; si no, `/run/user/<uid>` si existe.
/// - macOS: `XDG_RUNTIME_DIR` si es absoluto; si no, `$TMPDIR/ecosistema-asistente-ejecucion`
///   (el `TMPDIR` de macOS es privado del usuario).
/// - Windows: `%LOCALAPPDATA%\ecosistema-asistente\ejecucion`.
pub fn dir_ejecucion(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let absoluto = |nombre: &str| var(nombre).map(PathBuf::from).filter(|d| d.is_absolute());
    #[cfg(windows)]
    {
        absoluto("LOCALAPPDATA").map(|d| d.join("ecosistema-asistente").join("ejecucion"))
    }
    #[cfg(not(windows))]
    {
        if let Some(d) = absoluto("XDG_RUNTIME_DIR") {
            return Some(d);
        }
        #[cfg(target_os = "macos")]
        {
            absoluto("TMPDIR").map(|d| d.join("ecosistema-asistente-ejecucion"))
        }
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(unix)]
            {
                // SAFETY: `getuid` no tiene precondiciones.
                let uid = unsafe { libc::getuid() };
                let d = PathBuf::from(format!("/run/user/{uid}"));
                d.is_dir().then_some(d)
            }
            #[cfg(not(unix))]
            {
                None
            }
        }
    }
}

/// Extensiones (en minúsculas, con punto) que cuentan como ejecutables en
/// Windows: las de `PATHEXT` o, si no está o está vacío, `.exe .cmd .bat .com`.
pub fn extensiones_ejecutables(pathext: Option<&OsStr>) -> Vec<String> {
    let de_entorno: Vec<String> = pathext
        .map(|p| p.to_string_lossy().split(';').map(|e| e.trim().to_ascii_lowercase()).filter(|e| e.starts_with('.') && e.len() > 1).collect())
        .unwrap_or_default();
    if de_entorno.is_empty() { [".exe", ".cmd", ".bat", ".com"].map(String::from).to_vec() } else { de_entorno }
}

/// Busca un programa. Si `nombre` ya trae directorio, solo se comprueba esa
/// ruta; si no, se recorre `path` (con `env::split_paths`). En Windows se
/// prueba también `nombre` + cada extensión de `pathext`. Devuelve la primera
/// coincidencia que sea ejecutable.
pub fn buscar_binario(nombre: &str, path: Option<&OsStr>, pathext: Option<&OsStr>) -> Option<PathBuf> {
    if nombre.is_empty() {
        return None;
    }
    let candidatos = |base: &Path| -> Vec<PathBuf> {
        let mut v = vec![base.to_path_buf()];
        if cfg!(windows) {
            for e in extensiones_ejecutables(pathext) {
                let mut s: OsString = base.as_os_str().to_owned();
                s.push(&e);
                v.push(PathBuf::from(s));
            }
        }
        v
    };
    let p = Path::new(nombre);
    if p.components().count() > 1 || p.is_absolute() {
        return candidatos(p).into_iter().find(|c| crate::fs::es_ejecutable(c));
    }
    for dir in std::env::split_paths(path?) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        if let Some(c) = candidatos(&dir.join(nombre)).into_iter().find(|c| crate::fs::es_ejecutable(c)) {
            return Some(c);
        }
    }
    None
}

/// [`buscar_binario`] con el `PATH` y `PATHEXT` del entorno actual.
pub fn buscar_en_path(nombre: &str) -> Option<PathBuf> {
    buscar_binario(nombre, std::env::var_os("PATH").as_deref(), std::env::var_os("PATHEXT").as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn crear_programa(dir: &Path, nombre: &str) -> PathBuf {
        let f = dir.join(nombre);
        fs::write(&f, "#!/bin/sh\n").unwrap();
        crate::fs::poner_permisos_unix(&f, 0o755).unwrap();
        f
    }

    #[test]
    fn busca_en_un_path_temporal() {
        let d = tempfile::tempdir().unwrap();
        let otro = tempfile::tempdir().unwrap();
        let prog = crear_programa(d.path(), &nombre_exe("miprog"));
        let path = std::env::join_paths([otro.path(), d.path()]).unwrap();
        assert_eq!(buscar_binario("miprog", Some(&path), None), Some(prog.clone()));
        assert_eq!(buscar_binario("nada", Some(&path), None), None);
        assert_eq!(buscar_binario("miprog", None, None), None);
        // Con ruta propia se comprueba directamente.
        assert_eq!(buscar_binario(prog.to_str().unwrap(), Some(OsStr::new("")), None), Some(prog));
    }

    #[cfg(unix)]
    #[test]
    fn un_fichero_sin_permiso_no_vale() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("suelto");
        fs::write(&f, "").unwrap();
        crate::fs::poner_permisos_unix(&f, 0o644).unwrap();
        assert_eq!(buscar_binario("suelto", Some(d.path().as_os_str()), None), None);
    }

    #[test]
    fn extensiones_por_defecto_y_de_pathext() {
        assert_eq!(extensiones_ejecutables(None), [".exe", ".cmd", ".bat", ".com"]);
        assert_eq!(extensiones_ejecutables(Some(OsStr::new(".COM;.EXE;;.PS1"))), [".com", ".exe", ".ps1"]);
    }

    #[test]
    fn dir_ejecucion_usa_xdg_si_es_absoluto() {
        if cfg!(windows) {
            let w = |k: &str| (k == "LOCALAPPDATA").then(|| "C:\\Users\\a\\AppData\\Local".to_string());
            assert_eq!(dir_ejecucion(w), Some(PathBuf::from("C:\\Users\\a\\AppData\\Local").join("ecosistema-asistente").join("ejecucion")));
        } else {
            let v = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| "/run/user/42".to_string());
            assert_eq!(dir_ejecucion(v), Some(PathBuf::from("/run/user/42")));
            // Una relativa no vale.
            let rel = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| "relativa".to_string());
            assert_ne!(dir_ejecucion(rel), Some(PathBuf::from("relativa")));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn en_macos_cae_a_tmpdir() {
        let v = |k: &str| (k == "TMPDIR").then(|| "/var/folders/x/T".to_string());
        assert_eq!(dir_ejecucion(v), Some(PathBuf::from("/var/folders/x/T/ecosistema-asistente-ejecucion")));
    }

    #[test]
    fn nombre_exe_y_separador() {
        assert_eq!(nombre_exe("ea"), if cfg!(windows) { "ea.exe" } else { "ea" });
        assert_eq!(SEPARADOR_PATH, if cfg!(windows) { ';' } else { ':' });
    }
}
