//! Las marcas de los subagentes de Claude Code: `<estado>/subagentes/<sesión>/<agente>.json`,
//! una por subagente, bajo la raíz del bus. Las escribe el hook
//! (`ea hook subagentes`, [`registrar`]) y las lee el panel ([`de_sesion`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::fs_atomico::escribir_atomico;

/// Las carpetas de sesión más viejas que esto se borran al arrancar un subagente.
const ANTIGUEDAD_MAXIMA: Duration = Duration::from_secs(3 * 24 * 3600);

/// Un subagente: su id, su tipo, la transcripción de la sesión que lo lanzó y
/// cuándo empezó y acabó (ms desde la época).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarcaSubagente {
    pub agente: String,
    #[serde(default)]
    pub tipo: String,
    #[serde(default)]
    pub transcript: String,
    #[serde(default)]
    pub desde: u64,
    /// `None` mientras sigue en marcha.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hasta: Option<u64>,
}

/// La carpeta de las marcas dentro de la raíz del bus.
pub fn dir(estado: &Path) -> PathBuf {
    estado.join("subagentes")
}

/// El identificador sirve de nombre de directorio o de fichero: nada de rutas.
fn id_seguro(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\\', '\0', ':']) && !s.contains("..")
}

/// Apunta un subagente de `sesion` en `dir`. Sin `hasta`, empieza: se escribe
/// `marca` tal cual y se borran las sesiones viejas. Con `hasta`, acaba: la
/// marca anterior gana ese `hasta` (si no había, se escribe `marca`). Mejor
/// esfuerzo: los errores se ignoran a propósito y un id que no sirve de nombre
/// de fichero no escribe nada.
pub fn registrar(dir: &Path, sesion: &str, marca: MarcaSubagente) {
    if !id_seguro(sesion) || !id_seguro(&marca.agente) {
        return;
    }
    let fichero = dir.join(sesion).join(format!("{}.json", marca.agente));
    let marca = match marca.hasta {
        None => {
            limpiar_antiguos(dir);
            marca
        }
        Some(hasta) => match std::fs::read(&fichero).ok().and_then(|b| serde_json::from_slice::<MarcaSubagente>(&b).ok()) {
            Some(previa) => MarcaSubagente { hasta: Some(hasta), ..previa },
            None => marca,
        },
    };
    if let Ok(bytes) = serde_json::to_vec(&marca) {
        let _ = escribir_atomico(&fichero, &bytes);
    }
}

/// Borra las carpetas de sesión sin cambios desde hace más de tres días.
fn limpiar_antiguos(dir: &Path) {
    let Ok(entradas) = std::fs::read_dir(dir) else { return };
    for e in entradas.flatten() {
        let vieja =
            e.metadata().ok().filter(|m| m.is_dir()).and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|d| d > ANTIGUEDAD_MAXIMA);
        if vieja {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Las marcas de una sesión, en el orden del directorio. Nunca falla: un
/// directorio ilegible o un fichero roto se ignoran.
pub fn de_sesion(dir: &Path, sesion: &str) -> Vec<MarcaSubagente> {
    if !id_seguro(sesion) {
        return Vec::new();
    }
    let Ok(entradas) = std::fs::read_dir(dir.join(sesion)) else { return Vec::new() };
    entradas
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marca(agente: &str, desde: u64, hasta: Option<u64>) -> MarcaSubagente {
        MarcaSubagente { agente: agente.into(), tipo: "Explore".into(), transcript: "/t/s.jsonl".into(), desde, hasta }
    }

    #[test]
    fn lo_que_apunta_el_hook_lo_lee_el_panel() {
        let d = tempfile::tempdir().unwrap();
        registrar(d.path(), "s1", marca("a1", 1000, None));
        registrar(d.path(), "s1", marca("a1", 2000, Some(2000)));
        std::fs::write(d.path().join("s1/roto.json"), "{").unwrap();
        std::fs::write(d.path().join("s1/otro.txt"), "{}").unwrap();
        assert_eq!(de_sesion(d.path(), "s1"), [marca("a1", 1000, Some(2000))]);
        assert!(de_sesion(d.path(), "../s1").is_empty());
        assert!(de_sesion(d.path(), "nadie").is_empty());
    }
}
