//! Skills del ejecutor por trabajo: `<estado>/trabajos/<id>/orden.json`
//! (`skills` de la orden y de cada fase). Solo cuenta un trabajo si el
//! ejecutor llegó a pasárselas (existe `skills-agente.md`, que es lo que
//! escribe el ejecutor al lanzar). El actor es siempre `ejecutor`.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use super::parse_ts;

/// `(skill, usos, último en ms)` sumando todos los trabajos.
pub fn leer(trabajos: &Path, avisos: &mut Vec<String>) -> Vec<(String, u64, Option<i64>)> {
    let mut acum: std::collections::BTreeMap<String, (u64, Option<i64>)> = Default::default();
    let Ok(rd) = std::fs::read_dir(trabajos) else { return Vec::new() };
    for e in rd.flatten() {
        let dir = e.path();
        let (orden, entregadas) = (dir.join("orden.json"), dir.join("skills-agente.md"));
        if !entregadas.is_file() || !orden.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&orden) else { continue };
        let Ok(v) = serde_json::from_slice::<Value>(&bytes) else {
            avisos.push(format!("{}: orden ilegible; sus skills no cuentan", orden.display()));
            continue;
        };
        let mut skills: BTreeSet<String> = BTreeSet::new();
        let mut añade = |a: Option<&Vec<Value>>| {
            for s in a.into_iter().flatten().filter_map(Value::as_str) {
                skills.insert(s.to_string());
            }
        };
        añade(v.get("skills").and_then(Value::as_array));
        for f in v.get("fases").and_then(Value::as_array).into_iter().flatten() {
            añade(f.get("skills").and_then(Value::as_array));
        }
        let ts = v
            .get("creada_en")
            .and_then(Value::as_str)
            .and_then(parse_ts)
            .or_else(|| {
                std::fs::metadata(&entregadas)
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64)
            });
        for s in skills {
            let a = acum.entry(s).or_insert((0, None));
            a.0 += 1;
            a.1 = a.1.max(ts);
        }
    }
    acum.into_iter().map(|(n, (u, t))| (n, u, t)).collect()
}
