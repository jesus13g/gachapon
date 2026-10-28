//! El directorio de ejecución de la sesión (`$XDG_RUNTIME_DIR`) y la clave
//! de sesión del panel web que vive en él. Lo comparten `ea-web` (que la
//! escribe), `ea-web-open` (que la lee) y el ejecutor (que tapa el
//! directorio dentro de `bwrap` para que el agente local no la lea).

use std::path::{Path, PathBuf};

use crate::fuentes::Entorno;

/// Subdirectorio de `ea-web` dentro del directorio de ejecución (0700).
pub const DIR_WEB: &str = "ea-web";
/// Fichero con la clave de sesión del panel (0600).
pub const FICHERO_CLAVE: &str = "clave";

/// El directorio de ejecución de la plataforma (Linux: `$XDG_RUNTIME_DIR` o
/// `/run/user/<uid>`; macOS: `$TMPDIR/…`; Windows: `%LOCALAPPDATA%\…`). El
/// entorno se lee a través de `entorno`. `None` si no hay ninguno.
pub fn dir_ejecucion(entorno: &Entorno) -> Option<PathBuf> {
    ea_plataforma::rutas::dir_ejecucion(|n| entorno.var(n))
}

/// Subdirectorio de `ea-escritorio` dentro del directorio de ejecución.
pub const DIR_ESCRITORIO: &str = "ea-escritorio";
/// Fichero de presencia del shell de escritorio (su pid; 0600).
pub const FICHERO_PRESENCIA: &str = "presencia";

/// Ruta del fichero de presencia de `ea-escritorio` dentro de un directorio
/// de ejecución: `<ejecucion>/ea-escritorio/presencia`.
pub fn fichero_presencia_escritorio(ejecucion: &Path) -> PathBuf {
    ejecucion.join(DIR_ESCRITORIO).join(FICHERO_PRESENCIA)
}

/// Ruta del fichero de la clave dentro de un directorio de ejecución.
pub fn fichero_clave_web(ejecucion: &Path) -> PathBuf {
    ejecucion.join(DIR_WEB).join(FICHERO_CLAVE)
}

/// ¿Es una clave de sesión del panel bien formada? 64 cifras hexadecimales:
/// lo que escribe `ea-web` y lo único que acepta la app de escritorio.
pub fn clave_web_valida(clave: &str) -> bool {
    clave.len() == 64 && clave.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_vale_una_clave_de_64_hex() {
        const BUENA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(clave_web_valida(BUENA));
        assert!(clave_web_valida(&BUENA.to_uppercase()));
        assert!(!clave_web_valida(""));
        assert!(!clave_web_valida(&BUENA[..63]));
        assert!(!clave_web_valida(&format!("{}z", &BUENA[..63])));
        assert!(!clave_web_valida(&format!("{BUENA}0")));
        assert!(!clave_web_valida(&format!("{BUENA}\n")));
    }

    #[cfg(unix)]
    #[test]
    fn el_directorio_sale_de_xdg_runtime_dir() {
        use std::collections::BTreeMap;
        let e = Entorno::Fijo(BTreeMap::from([("XDG_RUNTIME_DIR".into(), "/run/user/42".into())]));
        assert_eq!(dir_ejecucion(&e), Some(PathBuf::from("/run/user/42")));
        assert_eq!(fichero_clave_web(Path::new("/run/user/42")), PathBuf::from("/run/user/42/ea-web/clave"));
        assert_eq!(fichero_presencia_escritorio(Path::new("/run/user/42")), PathBuf::from("/run/user/42/ea-escritorio/presencia"));
        // Una ruta relativa no vale: se cae al de /run/user/<uid> (si existe).
        let rel = Entorno::Fijo(BTreeMap::from([("XDG_RUNTIME_DIR".into(), "relativa".into())]));
        assert_ne!(dir_ejecucion(&rel), Some(PathBuf::from("relativa")));
    }
}
