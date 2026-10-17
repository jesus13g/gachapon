//! Sesiones de Claude Code abiertas en la máquina, leídas de los ficheros
//! que Claude Code deja en `~/.claude/sessions/<pid>.json`, con sus
//! subagentes: los apunta el hook `claude/hooks/ea-subagentes.sh` en
//! `<subagentes_dir>/<sessionId>/<agente>.json`. Solo lectura.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Cuántos subagentes se muestran por sesión como mucho.
const TOPE_SUBAGENTES: usize = 8;

#[derive(Serialize)]
pub struct Sesion {
    pub pid: u32,
    pub nombre: String,
    pub cwd: String,
    pub ocupada: bool,
    pub desde: u64,
    pub actualizada: u64,
    pub subagentes: Vec<Subagente>,
}

#[derive(Serialize)]
pub struct Subagente {
    pub id: String,
    pub descripcion: String,
    pub tipo: String,
    pub modelo: String,
    pub desde: u64,
    /// `None` mientras sigue en marcha.
    pub hasta: Option<u64>,
}

#[derive(Deserialize)]
struct Fichero {
    pid: u32,
    #[serde(rename = "sessionId", default)]
    session_id: String,
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

/// Lo que escribe el hook.
#[derive(Deserialize)]
struct Marca {
    agente: String,
    #[serde(default)]
    tipo: String,
    #[serde(default)]
    transcript: String,
    #[serde(default)]
    desde: u64,
    #[serde(default)]
    hasta: Option<u64>,
}

/// El `meta.json` que Claude Code deja junto a la transcripción del subagente.
#[derive(Deserialize, Default)]
struct Meta {
    #[serde(default)]
    description: String,
    #[serde(default)]
    model: String,
}

/// Las sesiones cuyo proceso sigue vivo, de la más antigua a la más reciente.
/// Nunca falla: un directorio ilegible o un fichero roto se ignoran.
pub fn vivas(dir: &Path, subagentes_dir: Option<&Path>) -> Vec<Sesion> {
    let Ok(entradas) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Sesion> = entradas
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Fichero>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|f| Path::new("/proc").join(f.pid.to_string()).exists())
        .map(|f| Sesion {
            pid: f.pid,
            subagentes: subagentes_dir.map(|d| subagentes(d, &f.session_id)).unwrap_or_default(),
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

/// Los subagentes de una sesión: primero los que siguen en marcha (del más
/// antiguo al más nuevo), luego los terminados más recientes.
fn subagentes(dir: &Path, sesion: &str) -> Vec<Subagente> {
    if sesion.is_empty() || sesion.contains('/') || sesion.contains("..") {
        return Vec::new();
    }
    let Ok(entradas) = std::fs::read_dir(dir.join(sesion)) else { return Vec::new() };
    let mut v: Vec<Subagente> = entradas
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Marca>(&std::fs::read(e.path()).ok()?).ok())
        .map(|m| {
            let meta = meta(&m.transcript, &m.agente);
            Subagente {
                descripcion: if meta.description.is_empty() { m.tipo.clone() } else { meta.description },
                modelo: meta.model,
                id: m.agente,
                tipo: m.tipo,
                desde: m.desde,
                hasta: m.hasta,
            }
        })
        .collect();
    v.sort_by_key(|s| match s.hasta {
        None => (0, s.desde),
        Some(h) => (1, u64::MAX - h),
    });
    v.truncate(TOPE_SUBAGENTES);
    v
}

/// `<transcripción del padre sin .jsonl>/subagents/agent-<id>.meta.json`.
fn meta(transcript: &str, agente: &str) -> Meta {
    let Some(base) = transcript.strip_suffix(".jsonl") else { return Meta::default() };
    let ruta = Path::new(base).join("subagents").join(format!("agent-{agente}.meta.json"));
    std::fs::read(ruta).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
