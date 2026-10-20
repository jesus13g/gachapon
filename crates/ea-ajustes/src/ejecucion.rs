//! El directorio de ejecución de la sesión (`$XDG_RUNTIME_DIR`) y la clave
//! de sesión del panel web que vive en él. Lo comparten `ea-web` (que la
//! escribe), `ea-web-open` (que la lee) y el ejecutor (que tapa el
//! directorio dentro de `bwrap` para que el agente local no la lea).

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::fuentes::Entorno;

/// Subdirectorio de `ea-web` dentro del directorio de ejecución (0700).
pub const DIR_WEB: &str = "ea-web";
/// Fichero con la clave de sesión del panel (0600).
pub const FICHERO_CLAVE: &str = "clave";

/// `$XDG_RUNTIME_DIR` si está definido (y es absoluto); si no,
/// `/run/user/<uid>` cuando existe. `None` si no hay ninguno.
pub fn dir_ejecucion(entorno: &Entorno) -> Option<PathBuf> {
    if let Some(d) = entorno.var("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute()) {
        return Some(d);
    }
    let uid = std::fs::metadata("/proc/self").ok()?.uid();
    let d = PathBuf::from(format!("/run/user/{uid}"));
    d.is_dir().then_some(d)
}

/// Ruta del fichero de la clave dentro de un directorio de ejecución.
pub fn fichero_clave_web(ejecucion: &Path) -> PathBuf {
    ejecucion.join(DIR_WEB).join(FICHERO_CLAVE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn el_directorio_sale_de_xdg_runtime_dir() {
        let e = Entorno::Fijo(BTreeMap::from([("XDG_RUNTIME_DIR".into(), "/run/user/42".into())]));
        assert_eq!(dir_ejecucion(&e), Some(PathBuf::from("/run/user/42")));
        assert_eq!(fichero_clave_web(Path::new("/run/user/42")), PathBuf::from("/run/user/42/ea-web/clave"));
        // Una ruta relativa no vale: se cae al de /run/user/<uid> (si existe).
        let rel = Entorno::Fijo(BTreeMap::from([("XDG_RUNTIME_DIR".into(), "relativa".into())]));
        assert_ne!(dir_ejecucion(&rel), Some(PathBuf::from("relativa")));
    }
}
