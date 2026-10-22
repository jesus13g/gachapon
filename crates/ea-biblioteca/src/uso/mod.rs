//! Uso de cada elemento por actor (spec 018, RF-005 y RF-006).
//!
//! [`calcular_uso`] agrega, por elemento del inventario y por actor
//! (`ecosistema`, `ejecutor` o el id de un agente), cuántas veces se usó y
//! cuándo fue la última, a partir de:
//!
//! - `~/.claude.json` (`skillUsage`, `pluginUsage`, `agentLastUsed`);
//! - las transcripciones de Claude Code (`tool_use` de `Skill`, `mcp__*`,
//!   `Agent`/`Task`) y, si el formato lo permite, las de Qwen Code y la base
//!   de OpenCode;
//! - el diario del ecosistema (`mcp_*`) → elemento MCP del ecosistema;
//! - las skills que se pasaron al ejecutor en cada trabajo del bus;
//! - Codex y Antigravity: solo última actividad.
//!
//! Dos fuentes que miden lo mismo (los contadores de `~/.claude.json` y las
//! transcripciones) no se suman: por elemento y actor manda la mayor. Dentro
//! de una fuente sí se suma. De las transcripciones solo se conserva el
//! nombre de la herramienta, el actor y la fecha; la caché también.
//!
//! Limitación conocida: si una transcripción se copia en otra (sesiones
//! reanudadas), sus usos se cuentan en ambas.

mod cache;
mod claude;
mod codex;
mod diario;
mod ejecutor;
mod opencode;
mod qwen;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Inventario;
use crate::agentes::ecosistema::NOMBRE_MCP;
use crate::modelo::{AGENTE_ECOSISTEMA, Ambito, Elemento, Tipo};
use crate::raices::Raices;
use cache::{Cache, acumular};

/// Actor de lo que ejecuta el ejecutor (el agente local), sea cual sea.
pub const ACTOR_EJECUTOR: &str = "ejecutor";

/// Una llamada encontrada en una fuente. Nunca lleva contenido.
#[derive(Debug, Clone)]
pub(crate) struct Hallazgo {
    pub tipo: Tipo,
    pub nombre: String,
    pub ts_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsoActor {
    pub actor: String,
    pub usos: u64,
    /// RFC 3339 (UTC).
    pub ultimo: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Uso {
    pub id: String,
    pub total: u64,
    pub ultimo: Option<String>,
    pub por: Vec<UsoActor>,
}

/// Algo usado que no casa con ningún elemento del inventario (un MCP de
/// claude.ai, un subagente integrado como `Explore`…).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsoSinElemento {
    pub agente: String,
    pub tipo: Tipo,
    pub nombre: String,
    pub total: u64,
    pub ultimo: Option<String>,
    pub por: Vec<UsoActor>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActividadAgente {
    pub agente: String,
    pub ultima: Option<String>,
    /// De dónde sale la fecha.
    pub fuente: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Estadisticas {
    /// Ficheros de transcripción vistos.
    pub ficheros: u64,
    /// Releídos enteros (nuevos, encogidos o cambiados sin crecer).
    pub ficheros_releidos: u64,
    /// Leídos solo desde el offset guardado.
    pub ficheros_incrementales: u64,
    /// Líneas procesadas en esta pasada (no cuenta las que da la caché).
    pub lineas_leidas: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct InformeUso {
    /// Solo los elementos con algún uso, por id.
    pub usos: Vec<Uso>,
    pub sin_elemento: Vec<UsoSinElemento>,
    pub actividad: Vec<ActividadAgente>,
    pub avisos: Vec<String>,
    pub estadisticas: Estadisticas,
}

impl InformeUso {
    pub fn buscar(&self, id: &str) -> Option<&Uso> {
        self.usos.iter().find(|u| u.id == id)
    }
}

/// Dónde está cada fuente. Se inyecta para las pruebas.
#[derive(Debug, Clone)]
pub struct FuentesUso {
    pub claude_json: PathBuf,
    pub claude_proyectos: PathBuf,
    pub diario: PathBuf,
    /// `<estado>/trabajos`.
    pub trabajos: PathBuf,
    pub codex_estado: PathBuf,
    pub codex_sesiones: PathBuf,
    pub opencode_db: PathBuf,
    pub qwen_proyectos: PathBuf,
    pub antigravity_logs: PathBuf,
    /// `<estado>/biblioteca/cache-uso.json`; `None`: sin caché.
    pub cache: Option<PathBuf>,
}

impl FuentesUso {
    /// Las rutas estándar bajo `r.home` y `r.estado`. El diario es `r.db` (en
    /// el entorno real, `$EA_DB_PATH` o `rutas.db` de los ajustes). La de
    /// OpenCode sigue su convención XDG en los tres sistemas (relativa al home:
    /// `~/.local/share/opencode/opencode.db`, también en macOS y Windows).
    pub fn desde_raices(r: &Raices) -> Self {
        let h = &r.home;
        let diario = r.db.clone();
        FuentesUso {
            claude_json: h.join(".claude.json"),
            claude_proyectos: h.join(".claude/projects"),
            diario,
            trabajos: r.estado.join("trabajos"),
            codex_estado: h.join(".codex/state_5.sqlite"),
            codex_sesiones: h.join(".codex/sessions"),
            opencode_db: h.join(".local/share/opencode/opencode.db"),
            qwen_proyectos: h.join(".qwen/projects"),
            antigravity_logs: h.join(".gemini/antigravity-cli/log"),
            cache: Some(r.estado.join("biblioteca/cache-uso.json")),
        }
    }
}

pub(crate) fn parse_ts(t: &str) -> Option<i64> {
    OffsetDateTime::parse(t, &Rfc3339).ok().map(|d| (d.unix_timestamp_nanos() / 1_000_000) as i64)
}

pub fn formatear_ms(ms: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000).ok()?.format(&Rfc3339).ok()
}

/// Ficheros bajo `dir` (recursivo, sin seguir enlaces simbólicos) que cumplen el filtro de nombre.
fn ficheros_bajo(dir: &Path, admite: &dyn Fn(&str) -> bool, sal: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let p = e.path();
        if t.is_dir() {
            ficheros_bajo(&p, admite, sal);
        } else if t.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(admite) {
            sal.push(p);
        }
    }
}

/// El `mtime` más reciente (ms) de los ficheros que cumplen el filtro.
pub(crate) fn mtime_maximo(dir: &Path, admite: impl Fn(&str) -> bool) -> Option<i64> {
    let mut v = Vec::new();
    ficheros_bajo(dir, &admite, &mut v);
    v.iter().filter_map(|p| mtime_ms(p)).max()
}

fn mtime_ms(p: &Path) -> Option<i64> {
    let d = std::fs::metadata(p).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_millis() as i64)
}

// --- Casado con el inventario -------------------------------------------------

/// Como Claude Code nombra un servidor en `mcp__<servidor>__…`: lo que no es
/// alfanumérico, `_` ni `-` pasa a `_`.
fn normalizar_mcp(n: &str) -> String {
    n.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn plugin_corto(e: &Elemento) -> Option<&str> {
    match &e.ambito {
        Ambito::Plugin(id) => Some(id.split('@').next().unwrap_or(id)),
        _ => None,
    }
}

fn casa(e: &Elemento, tipo: Tipo, nombre: &str) -> bool {
    if e.tipo != tipo {
        return false;
    }
    match tipo {
        Tipo::Skill | Tipo::Subagente | Tipo::Comando => {
            e.nombre == nombre
                || nombre
                    .split_once(':')
                    .is_some_and(|(pfx, suf)| plugin_corto(e) == Some(pfx) && e.nombre == suf)
        }
        Tipo::Plugin | Tipo::Extension => e.nombre == nombre,
        Tipo::Mcp => {
            let n = normalizar_mcp(&e.nombre);
            n == nombre || plugin_corto(e).is_some_and(|p| nombre == format!("plugin_{}_{}", normalizar_mcp(p), n))
        }
    }
}

fn resolver<'a>(inv: &'a Inventario, agente: &str, tipo: Tipo, nombre: &str) -> Option<&'a Elemento> {
    let del_agente = inv.elementos.iter().filter(|e| e.agente == agente);
    if agente == "opencode" && tipo == Tipo::Mcp {
        // OpenCode llama a las herramientas MCP `<servidor>_<herramienta>`: gana el prefijo más largo.
        return del_agente
            .filter(|e| e.tipo == Tipo::Mcp && nombre.starts_with(&format!("{}_", normalizar_mcp(&e.nombre))))
            .max_by_key(|e| e.nombre.len());
    }
    del_agente.filter(|e| casa(e, tipo, nombre)).next()
}

// --- Acumulación ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Destino {
    Id(String),
    Sin(String, Tipo, String),
}

#[derive(Default)]
struct Acum {
    /// (destino, actor, fuente) → suma dentro de la fuente.
    por_fuente: BTreeMap<(Destino, String, &'static str), (u64, Option<i64>)>,
}

impl Acum {
    fn anota(&mut self, inv: &Inventario, agente: &str, tipo: Tipo, nombre: &str, actor: &str, fuente: &'static str, usos: u64, ultimo: Option<i64>) {
        let destino = match resolver(inv, agente, tipo, nombre) {
            Some(e) => Destino::Id(e.id.clone()),
            None => Destino::Sin(agente.to_string(), tipo, nombre.to_string()),
        };
        let e = self.por_fuente.entry((destino, actor.to_string(), fuente)).or_insert((0, None));
        e.0 += usos;
        e.1 = e.1.max(ultimo);
    }

    fn registros(&mut self, inv: &Inventario, agente: &str, actor: &str, fuente: &'static str, regs: &[cache::Reg]) {
        for r in regs {
            self.anota(inv, agente, r.tipo, &r.nombre, actor, fuente, r.usos, r.ultimo_ms);
        }
    }

    /// Entre fuentes distintas manda la mayor; devuelve destino → actor → (usos, último).
    fn consolidar(self) -> BTreeMap<Destino, BTreeMap<String, (u64, Option<i64>)>> {
        let mut sal: BTreeMap<Destino, BTreeMap<String, (u64, Option<i64>)>> = BTreeMap::new();
        for ((d, actor, _), (usos, ultimo)) in self.por_fuente {
            let e = sal.entry(d).or_default().entry(actor).or_insert((0, None));
            e.0 = e.0.max(usos);
            e.1 = e.1.max(ultimo);
        }
        sal
    }
}

fn por_actor(m: &BTreeMap<String, (u64, Option<i64>)>) -> (u64, Option<i64>, Vec<UsoActor>) {
    let total = m.values().map(|v| v.0).sum();
    let ultimo = m.values().filter_map(|v| v.1).max();
    let por = m.iter().map(|(a, (u, t))| UsoActor { actor: a.clone(), usos: *u, ultimo: t.and_then(formatear_ms) }).collect();
    (total, ultimo, por)
}

/// Calcula el uso. Lo que falta en disco no es error; lo que no se pudo leer
/// queda en `avisos`. Con `fuentes.cache` la lectura de las transcripciones
/// es incremental y se guarda la caché al terminar.
pub fn calcular_uso(_raices: &Raices, inv: &Inventario, fuentes: &FuentesUso) -> InformeUso {
    let mut avisos = Vec::new();
    let mut est = Estadisticas::default();
    let mut acum = Acum::default();
    let mut cache = fuentes.cache.as_deref().map(Cache::cargar).unwrap_or_default();
    let mut vivas: HashSet<String> = HashSet::new();
    let mut actividad: Vec<ActividadAgente> = Vec::new();

    // 1. ~/.claude.json
    if let Some(c) = claude::contadores(&fuentes.claude_json, &mut avisos) {
        for (n, u, t) in c.skills {
            acum.anota(inv, "claude-code", Tipo::Skill, &n, "claude-code", "claude-json", u, t);
        }
        for (n, u, t) in c.plugins {
            acum.anota(inv, "claude-code", Tipo::Plugin, &n, "claude-code", "claude-json", u, t);
        }
        for (n, u, t) in c.subagentes {
            acum.anota(inv, "claude-code", Tipo::Subagente, &n, "claude-code", "claude-json", u, t);
        }
    }

    // 2. Transcripciones de Claude Code y de Qwen Code.
    let mut grupos: Vec<(&str, &Path, fn(&str) -> Vec<Hallazgo>, &'static str)> = vec![
        ("claude-code", &fuentes.claude_proyectos, claude::extraer, "transcripciones"),
        ("qwen-code", &fuentes.qwen_proyectos, qwen::extraer, "transcripciones"),
    ];
    for (agente, dir, extraer, fuente) in grupos.drain(..) {
        let mut ficheros = Vec::new();
        ficheros_bajo(dir, &|n| n.ends_with(".jsonl"), &mut ficheros);
        ficheros.sort();
        let mut ultima_actividad: Option<i64> = None;
        for f in &ficheros {
            let clave = format!("{agente}:{}", f.display());
            vivas.insert(clave.clone());
            cache::escanear_lineas(&mut cache, &clave, f, extraer, &mut est, &mut avisos);
            ultima_actividad = ultima_actividad.max(mtime_ms(f));
            if let Some(en) = cache.fuentes.get(&clave) {
                acum.registros(inv, agente, agente, fuente, &en.regs);
            }
        }
        actividad.push(ActividadAgente {
            agente: agente.into(),
            ultima: ultima_actividad.and_then(formatear_ms),
            fuente: "mtime de las transcripciones".into(),
        });
    }

    // 3. OpenCode (incremental por time_created).
    {
        let clave = format!("opencode:{}", fuentes.opencode_db.display());
        vivas.insert(clave.clone());
        let previa = cache.fuentes.get(&clave).cloned().unwrap_or_default();
        let visto = opencode::Visto { marca: previa.offset as i64, ids: previa.vistos.clone() };
        let (hallazgos, visto) = opencode::leer(&fuentes.opencode_db, &visto, &mut avisos);
        let mut nuevos: BTreeMap<(Tipo, String), (u64, Option<i64>)> = BTreeMap::new();
        for h in hallazgos {
            let e = nuevos.entry((h.tipo, h.nombre)).or_insert((0, None));
            e.0 += 1;
            e.1 = e.1.max(h.ts_ms);
        }
        let mut en = previa;
        en.offset = visto.marca.max(0) as u64;
        en.vistos = visto.ids;
        acumular(&mut en.regs, nuevos);
        acum.registros(inv, "opencode", "opencode", "opencode", &en.regs);
        cache.fuentes.insert(clave, en);
        actividad.push(ActividadAgente {
            agente: "opencode".into(),
            ultima: opencode::actividad(&fuentes.opencode_db).and_then(formatear_ms),
            fuente: "última sesión de opencode.db".into(),
        });
    }

    // 4. Codex y Antigravity: solo actividad.
    actividad.push(ActividadAgente {
        agente: "codex".into(),
        ultima: codex::actividad_codex(&fuentes.codex_estado, &fuentes.codex_sesiones).and_then(formatear_ms),
        fuente: "hilos de state_5.sqlite y rollouts".into(),
    });
    actividad.push(ActividadAgente {
        agente: "antigravity".into(),
        ultima: codex::actividad_antigravity(&fuentes.antigravity_logs).and_then(formatear_ms),
        fuente: "mtime de log/cli-*.log".into(),
    });

    // 5. Diario del ecosistema → servidor MCP del ecosistema.
    for (actor, n, t) in diario::leer(&fuentes.diario, &mut avisos) {
        acum.anota(inv, AGENTE_ECOSISTEMA, Tipo::Mcp, NOMBRE_MCP, &actor, "diario", n, t);
    }

    // 6. Skills del ejecutor por trabajo.
    for (n, u, t) in ejecutor::leer(&fuentes.trabajos, &mut avisos) {
        acum.anota(inv, "qwen-code", Tipo::Skill, &n, ACTOR_EJECUTOR, "bus", u, t);
    }

    if let Some(ruta) = &fuentes.cache {
        cache.retener(&vivas);
        cache.guardar(ruta, &mut avisos);
    }

    // Consolidar y ordenar.
    let mut usos = Vec::new();
    let mut sin_elemento = Vec::new();
    for (destino, actores) in acum.consolidar() {
        let (total, ultimo, por) = por_actor(&actores);
        let ultimo = ultimo.and_then(formatear_ms);
        match destino {
            Destino::Id(id) => usos.push(Uso { id, total, ultimo, por }),
            Destino::Sin(agente, tipo, nombre) => sin_elemento.push(UsoSinElemento { agente, tipo, nombre, total, ultimo, por }),
        }
    }
    actividad.sort_by_key(|a| crate::orden_agente(&a.agente));
    InformeUso { usos, sin_elemento, actividad, avisos, estadisticas: est }
}
