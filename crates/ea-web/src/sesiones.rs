//! Sesiones de Claude Code abiertas en la máquina, leídas de los ficheros
//! que Claude Code deja en `~/.claude/sessions/<pid>.json`, con sus
//! subagentes: los apunta el hook (`ea hook subagentes`) en
//! `<subagentes_dir>/<sessionId>/<agente>.json` (`ea_bus::subagentes`). Solo
//! lectura.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Cuántos subagentes se muestran por sesión como mucho.
const TOPE_SUBAGENTES: usize = 8;

#[derive(Serialize)]
pub struct Sesion {
    pub pid: u32,
    /// El `sessionId` de Claude Code (vacío en ficheros sin él).
    pub sesion: String,
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

/// El `meta.json` que Claude Code deja junto a la transcripción del subagente.
#[derive(Deserialize, Default)]
struct Meta {
    #[serde(default)]
    description: String,
    #[serde(default)]
    model: String,
    /// Claude Code lo pone a `true` al acabar el subagente: sirve aunque el
    /// hook `SubagentStop` no llegara a apuntarlo.
    #[serde(default)]
    finished: bool,
}

/// Las sesiones cuyo proceso sigue vivo, de la más antigua a la más reciente.
/// Nunca falla: un directorio ilegible o un fichero roto se ignoran.
pub fn vivas(dir: &Path, subagentes_dir: Option<&Path>) -> Vec<Sesion> {
    let Ok(entradas) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<Sesion> = entradas
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Fichero>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|f| ea_plataforma::proceso::vive(f.pid))
        .map(|f| Sesion {
            pid: f.pid,
            subagentes: subagentes_dir.map(|d| subagentes(d, &f.session_id)).unwrap_or_default(),
            sesion: f.session_id,
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
    let mut v: Vec<Subagente> = ea_bus::subagentes::de_sesion(dir, sesion)
        .into_iter()
        // Un `SubagentStop` sin `SubagentStart` ni meta.json (agentes internos
        // de Claude Code) deja una marca sin tipo ni descripción: no hay nada
        // que mostrar, así que no cuenta como subagente.
        .filter(|m| !m.tipo.is_empty() || !meta(&m.transcript, &m.agente).0.description.is_empty())
        .map(|m| {
            let (meta, acabado) = meta(&m.transcript, &m.agente);
            // Sin marca de fin pero con `finished`, la hora del meta.json.
            let hasta = m.hasta.or(if meta.finished { Some(acabado.unwrap_or(m.desde)) } else { None });
            Subagente {
                descripcion: if meta.description.is_empty() { m.tipo.clone() } else { meta.description },
                modelo: meta.model,
                id: m.agente,
                tipo: m.tipo,
                desde: m.desde,
                hasta,
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

/// `<transcripción del padre sin .jsonl>/subagents/agent-<id>.meta.json`, con
/// la hora (ms) de su última escritura.
fn meta(transcript: &str, agente: &str) -> (Meta, Option<u64>) {
    let Some(base) = transcript.strip_suffix(".jsonl") else { return (Meta::default(), None) };
    let ruta = Path::new(base).join("subagents").join(format!("agent-{agente}.meta.json"));
    let meta = std::fs::read(&ruta).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let cuando =
        std::fs::metadata(&ruta).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64);
    (meta, cuando)
}

/// El último modelo que aparece en la transcripción de una sesión
/// (`<proyectos>/<cwd con lo no alfanumérico como '-'>/<sesión>.jsonl`). Solo
/// se leen los últimos 64 KiB: la transcripción puede pesar megas.
pub fn modelo_sesion(proyectos: &Path, cwd: &str, sesion: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    if sesion.is_empty() || sesion.contains(['/', '\\']) || sesion.contains("..") {
        return None;
    }
    let dir: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let mut f = std::fs::File::open(proyectos.join(dir).join(format!("{sesion}.jsonl"))).ok()?;
    let largo = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(largo.saturating_sub(64 * 1024))).ok()?;
    let mut cola = Vec::new();
    f.read_to_end(&mut cola).ok()?;
    let texto = String::from_utf8_lossy(&cola);
    const CLAVE: &str = "\"model\":\"";
    // El último que sea un modelo: `<synthetic>` y similares no lo son.
    texto.rmatch_indices(CLAVE).find_map(|(i, _)| {
        let resto = &texto[i + CLAVE.len()..];
        let modelo = &resto[..resto.find('"')?];
        (!modelo.is_empty() && !modelo.starts_with('<')).then(|| modelo.to_string())
    })
}
