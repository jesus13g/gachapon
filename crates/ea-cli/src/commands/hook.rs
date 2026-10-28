//! `ea hook subagentes <start|stop>`: el hook de Claude Code para
//! `SubagentStart` / `SubagentStop` (portable: sin `sh`, `jq` ni `date`).
//!
//! Lee el JSON del hook por la entrada estándar y deja un fichero por
//! subagente en `<estado>/subagentes/<sesión>/<agente>.json`
//! (`ea_bus::subagentes`), donde ea-web sabe qué subagentes siguen en marcha.
//! `<estado>` es el mismo que usa el resto del ecosistema
//! (`ea_bus::raiz_estado`: `$EA_ESTADO_DIR` o `rutas.estado` de los ajustes).
//!
//!   start → {"agente","tipo","transcript","desde"}
//!   stop  → añade "hasta" (crea el fichero si faltaba)
//!
//! Nunca falla ni escribe en la salida: un hook roto no puede estorbar a
//! Claude Code, así que cualquier error termina en silencio con código 0.

use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use ea_bus::subagentes::{self, MarcaSubagente};
use serde_json::Value;

pub fn subagentes(accion: &str) -> anyhow::Result<()> {
    let mut entrada = String::new();
    if std::io::stdin().read_to_string(&mut entrada).is_err() {
        return Ok(());
    }
    let ahora = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    registrar(&subagentes::dir(&ea_bus::raiz_estado()), accion, &entrada, ahora);
    Ok(())
}

/// Traduce el JSON del hook a una marca; los errores se ignoran a propósito.
pub(crate) fn registrar(raiz: &Path, accion: &str, entrada: &str, ahora: u64) {
    let Ok(v) = serde_json::from_str::<Value>(entrada) else { return };
    let texto = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let hasta = match accion {
        "start" => None,
        "stop" => Some(ahora),
        _ => return,
    };
    let marca = MarcaSubagente {
        agente: texto("agent_id"),
        tipo: texto("agent_type"),
        transcript: texto("transcript_path"),
        desde: ahora,
        hasta,
    };
    subagentes::registrar(raiz, &texto("session_id"), marca);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn la_marca_en_disco_es_siempre_la_misma_byte_a_byte() {
        let d = tempfile::tempdir().unwrap();
        let bytes = |a: &str| std::fs::read_to_string(d.path().join("s1").join(format!("{a}.json"))).unwrap();
        registrar(d.path(), "start", &entrada("s1", "a1"), 1000);
        assert_eq!(bytes("a1"), r#"{"agente":"a1","tipo":"Explore","transcript":"/t/s.jsonl","desde":1000}"#);
        registrar(d.path(), "stop", &entrada("s1", "a1"), 2000);
        assert_eq!(bytes("a1"), r#"{"agente":"a1","tipo":"Explore","transcript":"/t/s.jsonl","desde":1000,"hasta":2000}"#);
        registrar(d.path(), "stop", &entrada("s1", "a1"), 3000);
        assert_eq!(bytes("a1"), r#"{"agente":"a1","tipo":"Explore","transcript":"/t/s.jsonl","desde":1000,"hasta":3000}"#);
        registrar(d.path(), "stop", &entrada("s1", "a2"), 5000);
        assert_eq!(bytes("a2"), r#"{"agente":"a2","tipo":"Explore","transcript":"/t/s.jsonl","desde":5000,"hasta":5000}"#);
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
