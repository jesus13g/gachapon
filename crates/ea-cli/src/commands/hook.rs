//! `ea hook subagentes <start|stop>`: el hook de Claude Code para
//! `SubagentStart` / `SubagentStop` (portable: sin `sh`, `jq` ni `date`).
//!
//! Lee el JSON del hook por la entrada estándar y deja un fichero por
//! subagente en `<estado>/subagentes/<sesión>/<agente>.json`, donde ea-web
//! sabe qué subagentes siguen en marcha. `<estado>` es el mismo que usa el
//! resto del ecosistema (`ea_bus::raiz_estado`: `$EA_ESTADO_DIR` o
//! `rutas.estado` de los ajustes).
//!
//!   start → {"agente","tipo","transcript","desde"}
//!   stop  → añade "hasta" (crea el fichero si faltaba)
//!
//! Nunca falla ni escribe en la salida: un hook roto no puede estorbar a
//! Claude Code, así que cualquier error termina en silencio con código 0.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Las carpetas de sesión más viejas que esto se borran al arrancar un subagente.
const ANTIGUEDAD_MAXIMA: Duration = Duration::from_secs(3 * 24 * 3600);

pub fn subagentes(accion: &str) -> anyhow::Result<()> {
    let mut entrada = String::new();
    if std::io::stdin().read_to_string(&mut entrada).is_err() {
        return Ok(());
    }
    let ahora = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    registrar(&ea_bus::raiz_estado().join("subagentes"), accion, &entrada, ahora);
    Ok(())
}

/// El identificador sirve de nombre de directorio o de fichero: nada de rutas.
fn id_seguro(s: &str) -> bool {
    !s.is_empty() && !s.contains(['/', '\\', '\0', ':']) && !s.contains("..")
}

/// Hace el trabajo del hook; los errores se ignoran a propósito.
pub(crate) fn registrar(raiz: &Path, accion: &str, entrada: &str, ahora: u64) {
    let Ok(v) = serde_json::from_str::<Value>(entrada) else { return };
    let texto = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("");
    let (sesion, agente) = (texto("session_id"), texto("agent_id"));
    if !id_seguro(sesion) || !id_seguro(agente) {
        return;
    }
    let fichero = raiz.join(sesion).join(format!("{agente}.json"));
    let nuevo = |desde: u64, hasta: Option<u64>| {
        let mut m = json!({
            "agente": agente,
            "tipo": texto("agent_type"),
            "transcript": texto("transcript_path"),
            "desde": desde,
        });
        if let Some(h) = hasta {
            m["hasta"] = json!(h);
        }
        m
    };
    let marca = match accion {
        "start" => {
            limpiar_antiguos(raiz);
            nuevo(ahora, None)
        }
        "stop" => match std::fs::read(&fichero).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            Some(mut previa) if previa.is_object() => {
                previa["hasta"] = json!(ahora);
                previa
            }
            _ => nuevo(ahora, Some(ahora)),
        },
        _ => return,
    };
    if let Ok(bytes) = serde_json::to_vec(&marca) {
        let _ = ea_bus::escribir_atomico(&fichero, &bytes);
    }
}

/// Borra las carpetas de sesión sin cambios desde hace más de tres días.
fn limpiar_antiguos(raiz: &Path) {
    let Ok(entradas) = std::fs::read_dir(raiz) else { return };
    for e in entradas.flatten() {
        let vieja = e
            .metadata()
            .ok()
            .filter(|m| m.is_dir())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d > ANTIGUEDAD_MAXIMA);
        if vieja {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entrada(sesion: &str, agente: &str) -> String {
        json!({"session_id": sesion, "agent_id": agente, "agent_type": "Explore", "transcript_path": "/t/s.jsonl"}).to_string()
    }

    fn leer(raiz: &Path, s: &str, a: &str) -> Value {
        serde_json::from_slice(&std::fs::read(raiz.join(s).join(format!("{a}.json"))).unwrap()).unwrap()
    }

    #[test]
    fn start_y_stop_dejan_la_marca() {
        let d = tempfile::tempdir().unwrap();
        registrar(d.path(), "start", &entrada("s1", "a1"), 1000);
        assert_eq!(leer(d.path(), "s1", "a1"), json!({"agente":"a1","tipo":"Explore","transcript":"/t/s.jsonl","desde":1000}));
        registrar(d.path(), "stop", &entrada("s1", "a1"), 2000);
        assert_eq!(leer(d.path(), "s1", "a1")["hasta"], 2000);
        assert_eq!(leer(d.path(), "s1", "a1")["desde"], 1000);
    }

    #[test]
    fn stop_sin_start_crea_la_marca() {
        let d = tempfile::tempdir().unwrap();
        registrar(d.path(), "stop", &entrada("s1", "a2"), 5000);
        let m = leer(d.path(), "s1", "a2");
        assert_eq!((m["desde"].as_u64(), m["hasta"].as_u64()), (Some(5000), Some(5000)));
    }

    #[test]
    fn entradas_malas_no_escriben_nada() {
        let d = tempfile::tempdir().unwrap();
        registrar(d.path(), "start", "no es json", 1);
        registrar(d.path(), "start", &entrada("../x", "a"), 1);
        registrar(d.path(), "start", &entrada("s", "a/b"), 1);
        registrar(d.path(), "start", &entrada("", "a"), 1);
        registrar(d.path(), "otra", &entrada("s", "a"), 1);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }
}
