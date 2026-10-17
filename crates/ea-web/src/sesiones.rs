//! Sesiones de Claude Code abiertas en la máquina, leídas de los ficheros
//! que Claude Code deja en `~/.claude/sessions/<pid>.json`. Solo lectura.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Sesion {
    pub pid: u32,
    pub nombre: String,
    pub cwd: String,
    pub ocupada: bool,
    pub desde: u64,
    pub actualizada: u64,
}

#[derive(Deserialize)]
struct Fichero {
    pid: u32,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    status: String,
    #[serde(rename = "startedAt", default)]
    started_at: u64,
    #[serde(rename = "updatedAt", default)]
    updated_at: u64,
}

/// Las sesiones cuyo proceso sigue vivo, de la más antigua a la más reciente.
/// Nunca falla: un directorio ilegible o un fichero roto se ignoran.
pub fn vivas(dir: &Path) -> Vec<Sesion> {
    let Ok(entradas) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Sesion> = entradas
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Fichero>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|f| Path::new("/proc").join(f.pid.to_string()).exists())
        .map(|f| Sesion {
            pid: f.pid,
            nombre: f.name.unwrap_or_default(),
            cwd: f.cwd,
            ocupada: f.status == "busy",
            desde: f.started_at,
            actualizada: f.updated_at,
        })
        .collect();
    v.sort_by_key(|s| s.desde);
    v
}
