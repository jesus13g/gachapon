//! El ecosistema vivo para la escena de Inicio (spec 010, escena viva): qué
//! agentes hay activos, de qué familia es cada uno, quién cuelga de quién y
//! dónde está cada trabajo activo. `GET /api/ecosistema` lo sirve tal cual.
//!
//! Tres capas:
//! - **detectores** (`claude`, `bus`): cada uno convierte una fuente real en
//!   [`AgenteVivo`] normalizados. Un agente nuevo con su propia fuente es un
//!   detector más; el navegador no sabe de dónde salió cada máquina.
//! - **[`componer`]**: junta lo detectado, quita duplicados y decide, solo con
//!   datos del bus, dónde está cada trabajo ([`Tarea`]) y de qué máquina sale
//!   cada evento reciente del diario ([`Senal`]).
//! - el navegador (`gashapon.js`) lo dibuja.
//!
//! Todo es de solo lectura y de mejor esfuerzo: una fuente ilegible no da
//! agentes, nunca un error.

pub mod bus;
pub mod claude;
pub mod familia;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EstadoAgente {
    /// Encendido y con algo entre manos (sesión ocupada, trabajo cogido,
    /// subagente en marcha).
    Trabajando,
    /// Encendido, sin nada que hacer ahora.
    Esperando,
    /// Acaba de terminar (un subagente): se apaga y sale de la escena.
    Terminado,
}

/// Un agente activo, venga de la fuente que venga.
#[derive(Debug, Clone, Serialize)]
pub struct AgenteVivo {
    /// Estable entre sondeos: la escena lo usa como clave de su máquina.
    pub id: String,
    /// Id del agente en el catálogo (`claude-code`, `qwen-code`…), si se sabe.
    pub agente: Option<String>,
    /// Nombre para la placa: el del catálogo o el tipo de subagente.
    pub nombre: String,
    /// La variante visual (ver [`familia::familia`]).
    pub familia: &'static str,
    pub modelo: Option<String>,
    /// Su modelo corre en esta máquina.
    pub local: bool,
    /// `planificador`, `ejecutor`, `revisor` o `subagente`.
    pub rol: Option<String>,
    /// El agente que lo lanzó. Puede encadenarse (subagentes de subagentes).
    pub padre: Option<String>,
    pub estado: EstadoAgente,
    /// Qué hace, en una línea: la sesión, la descripción del subagente o el
    /// título del trabajo.
    pub actividad: Option<String>,
    /// El trabajo del bus que tiene cogido, si alguno.
    pub trabajo: Option<String>,
    /// Directorio en el que trabaja (el cwd de la sesión).
    pub proyecto: Option<String>,
    /// Desde cuándo está activo, en ms desde la época.
    pub desde: Option<u64>,
    /// Qué detector lo vio: `claude-code`, `subagente`, `presencia` o `semaforo`.
    pub fuente: &'static str,
    pub equipo: Option<String>,
    /// Lo que permite reconocer sus trabajos (cwd e id de sesión: el worktree
    /// de un trabajo suele colgar de uno o llevar el otro). No sale en la API.
    #[serde(skip)]
    pub pistas: Vec<String>,
}

/// Dónde está ahora un trabajo activo y entre qué máquinas viaja.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tarea {
    pub trabajo: String,
    pub estado: String,
    /// Id de la máquina que lo tiene, `usuario` (espera tu decisión) o `bus`
    /// (en el raíl: no hay una máquina inequívoca para él).
    pub lugar: String,
    /// La máquina que lo planifica o lo revisa, si se sabe cuál es.
    pub planificador: Option<String>,
    /// La máquina que lo ejecuta (o lo ejecutará), si está encendida.
    pub ejecutor: Option<String>,
    /// Lo lanzó el usuario (una rutina), no un planificador.
    pub del_usuario: bool,
}

/// Un evento reciente del diario, con la máquina de la que sale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Senal {
    pub id: i64,
    /// Id de máquina, `usuario`, o `None` si el actor no corresponde a una
    /// sola máquina viva (sale del orquestador).
    pub de: Option<String>,
    pub actor: String,
    pub accion: String,
    pub trabajo: Option<String>,
    pub error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ecosistema {
    pub agentes: Vec<AgenteVivo>,
    pub tareas: Vec<Tarea>,
    pub senales: Vec<Senal>,
}

/// Lo que hace falta saber de un trabajo del bus para ubicarlo.
#[derive(Debug, Clone, Default)]
pub struct TrabajoBus {
    pub id: String,
    pub estado: String,
    pub titulo: String,
    pub worktree: String,
    pub asignado_a: Option<String>,
    pub revisor: Option<String>,
    /// Agente que lo tomó por última vez (`ejecutado_por`).
    pub ejecutado_por: Option<String>,
    pub del_usuario: bool,
}

/// Un semáforo cogido: qué agente del catálogo tiene qué trabajo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemaforoBus {
    pub agente: String,
    pub trabajo: String,
}

#[derive(Debug, Clone)]
pub struct EventoBus {
    pub id: i64,
    pub actor: String,
    pub accion: String,
    pub trabajo: Option<String>,
    pub error: bool,
}

/// Los agentes de los roles según los ajustes.
#[derive(Debug, Clone)]
pub struct Roles {
    pub planificador: String,
    pub ejecutor: String,
}

/// Agentes con un detector propio de sesiones: su presencia en el bus no es
/// una máquina aparte, sino un dato más de su sesión.
const CON_SESIONES: &[&str] = &["claude-code"];

/// Estados de un trabajo que tienen lugar en una máquina.
const ACTIVOS: &[&str] = &["en_cola", "ejecutando", "en_revision", "en_espera"];

/// Junta lo que dieron los detectores y lo relaciona con el bus.
pub fn componer(mut agentes: Vec<AgenteVivo>, trabajos: &[TrabajoBus], semaforos: &[SemaforoBus], eventos: &[EventoBus], roles: &Roles) -> Ecosistema {
    fundir_presencias(&mut agentes);

    // Lo que dicen los semáforos y los trabajos de cada ejecutor.
    for sem in semaforos {
        let titulo = trabajos.iter().find(|t| t.id == sem.trabajo).map(|t| t.titulo.clone());
        if let Some(a) = agentes.iter_mut().find(|a| a.padre.is_none() && a.agente.as_deref() == Some(&sem.agente) && a.fuente != "claude-code") {
            a.estado = EstadoAgente::Trabajando;
            a.trabajo = Some(sem.trabajo.clone());
            if titulo.is_some() {
                a.actividad = titulo;
            }
        }
    }
    for a in agentes.iter_mut().filter(|a| a.fuente == "presencia") {
        if let Some(t) = a.trabajo.as_ref().and_then(|id| trabajos.iter().find(|t| &t.id == id)) {
            a.actividad = Some(t.titulo.clone());
        }
    }

    let tareas = trabajos.iter().filter(|t| ACTIVOS.contains(&t.estado.as_str())).map(|t| ubicar(t, &agentes, semaforos, roles)).collect();
    let senales = eventos.iter().map(|e| senal(e, &agentes, roles)).collect();
    agentes.sort_by_key(orden);
    Ecosistema { agentes, tareas, senales }
}

/// Orden estable de la escena: primero los planificadores, luego los demás;
/// dentro, el más antiguo primero.
fn orden(a: &AgenteVivo) -> (u8, u64, String) {
    let peso = match a.rol.as_deref() {
        Some("planificador") => 0,
        Some("revisor") => 1,
        Some("ejecutor") => 2,
        _ => 3,
    };
    (peso, a.desde.unwrap_or(u64::MAX), a.id.clone())
}

/// Quita la presencia de los agentes que ya tienen sesión detectada; si hay
/// una sola sesión de ese agente, le pasa el rol y el equipo de la presencia.
fn fundir_presencias(agentes: &mut Vec<AgenteVivo>) {
    let presencias: Vec<AgenteVivo> =
        agentes.iter().filter(|a| a.fuente == "presencia" && a.agente.as_deref().is_some_and(|x| CON_SESIONES.contains(&x))).cloned().collect();
    agentes.retain(|a| !(a.fuente == "presencia" && a.agente.as_deref().is_some_and(|x| CON_SESIONES.contains(&x))));
    for p in presencias {
        let mut sesiones = agentes.iter_mut().filter(|a| a.padre.is_none() && a.agente == p.agente);
        if let (Some(s), None) = (sesiones.next(), sesiones.next()) {
            s.rol = p.rol;
            s.equipo = p.equipo;
        }
    }
}

/// Las máquinas principales de un agente del catálogo.
fn principales<'a>(agentes: &'a [AgenteVivo], agente: &'a str) -> impl Iterator<Item = &'a AgenteVivo> {
    agentes.iter().filter(move |a| a.padre.is_none() && a.agente.as_deref() == Some(agente))
}

/// La máquina de un agente del catálogo, si hay una sola encendida o si una
/// de ellas reconoce el worktree como suyo (la que lo reconoce más de cerca).
fn maquina_de(agentes: &[AgenteVivo], agente: &str, worktree: &str) -> Option<String> {
    let candidatas: Vec<&AgenteVivo> = principales(agentes, agente).collect();
    let afinidad = |a: &AgenteVivo| {
        a.pistas
            .iter()
            .filter(|p| !p.is_empty() && (cuelga_de(worktree, p) || (!p.contains('/') && worktree.split('/').any(|seg| seg == p.as_str()))))
            .map(String::len)
            .max()
    };
    if let Some(mejor) = candidatas.iter().filter_map(|a| afinidad(a).map(|n| (n, *a))).max_by_key(|(n, _)| *n) {
        return Some(mejor.1.id.clone());
    }
    match candidatas.as_slice() {
        [una] => Some(una.id.clone()),
        _ => None,
    }
}

/// `ruta` es `base` o cuelga de ella.
fn cuelga_de(ruta: &str, base: &str) -> bool {
    let base = base.trim_end_matches('/');
    base.starts_with('/') && (ruta == base || ruta.strip_prefix(base).is_some_and(|r| r.starts_with('/')))
}

fn ubicar(t: &TrabajoBus, agentes: &[AgenteVivo], semaforos: &[SemaforoBus], roles: &Roles) -> Tarea {
    let en_semaforo = semaforos.iter().find(|s| s.trabajo == t.id).map(|s| s.agente.as_str());
    let agente_ejecutor =
        t.asignado_a.as_deref().or(en_semaforo).or(if t.estado == "ejecutando" { t.ejecutado_por.as_deref() } else { None }).unwrap_or(&roles.ejecutor);
    let ejecutor = maquina_de(agentes, agente_ejecutor, &t.worktree);
    let planificador = match &t.revisor {
        Some(r) => maquina_de(agentes, r, &t.worktree),
        None => maquina_de(agentes, &roles.planificador, &t.worktree),
    };
    let lugar = match t.estado.as_str() {
        "en_cola" | "ejecutando" => ejecutor.clone(),
        "en_revision" => planificador.clone(),
        _ => Some("usuario".into()),
    };
    Tarea { trabajo: t.id.clone(), estado: t.estado.clone(), lugar: lugar.unwrap_or_else(|| "bus".into()), planificador, ejecutor, del_usuario: t.del_usuario }
}

/// De qué máquina sale un evento: la única máquina principal del agente que
/// lo firma. El actor puede ser el id del agente o el nombre de su rol en
/// disco (`claude-code` es también la cola del planificador).
fn senal(e: &EventoBus, agentes: &[AgenteVivo], roles: &Roles) -> Senal {
    let de = if e.actor == "usuario" {
        Some("usuario".into())
    } else {
        let agente = match ea_ajustes::Rol::desde(&e.actor) {
            Some(ea_ajustes::Rol::Ejecutor) => roles.ejecutor.as_str(),
            // `claude-code` es a la vez un agente y el rol en disco: si ese
            // agente tiene máquina, el evento es suyo.
            Some(ea_ajustes::Rol::Planificador) if principales(agentes, &e.actor).next().is_none() => roles.planificador.as_str(),
            _ => e.actor.as_str(),
        };
        let mut m = principales(agentes, agente);
        match (m.next(), m.next()) {
            (Some(a), None) => Some(a.id.clone()),
            _ => None,
        }
    };
    Senal { id: e.id, de, actor: e.actor.clone(), accion: e.accion.clone(), trabajo: e.trabajo.clone(), error: e.error }
}

#[cfg(test)]
mod tests;
