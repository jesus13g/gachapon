//! Registro único de claves: la API, la CLI y la página salen de aquí. Cada
//! clave dice su tipo y rango, quién la puede cambiar, cuándo se aplica, qué
//! variable de entorno la fija y de qué fichero antiguo se hereda.

use serde_json::Value;

use crate::agentes::{ficha, id_perfil_valido, IDS_AGENTES};
use crate::fuentes::{Ajustes, ConfigAgente, PerfilModelo};
use crate::validar::expandir_tilde;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    Booleano,
    Entero { min: i64, max: i64 },
    Texto { max: usize },
    /// URL del modelo: solo loopback.
    Url,
    /// Ruta de fichero o directorio; admite `~`.
    Ruta,
    /// Lista de nombres; con `permitidos`, solo esos.
    Lista { permitidos: Option<&'static [&'static str]> },
    /// Uno de una lista cerrada.
    Eleccion { permitidos: &'static [&'static str] },
    /// URL de un perfil de modelo: vacía o `http(s)://` sin credenciales. Que
    /// sea de loopback se exige al perfil no remoto (validación cruzada).
    UrlPerfil,
    /// Id de un perfil de modelo, o vacío.
    Perfil,
    /// Tabla de texto a texto (`{ "clave" = "valor" }`); en la CLI, JSON.
    Mapa,
}

impl Tipo {
    pub fn nombre(self) -> &'static str {
        match self {
            Tipo::Booleano => "booleano",
            Tipo::Entero { .. } => "entero",
            Tipo::Texto { .. } => "texto",
            Tipo::Url => "url",
            Tipo::Ruta => "ruta",
            Tipo::Lista { .. } => "lista",
            Tipo::Eleccion { .. } => "eleccion",
            Tipo::UrlPerfil => "url",
            Tipo::Perfil => "perfil",
            Tipo::Mapa => "mapa",
        }
    }
}

/// Desde dónde se puede cambiar una clave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edicion {
    /// Desde la página (⚙ Ajustes) y desde la terminal.
    Web,
    /// Solo con `ea ajustes establecer` en una terminal real.
    Terminal,
}

impl Edicion {
    pub fn nombre(self) -> &'static str {
        match self {
            Edicion::Web => "web",
            Edicion::Terminal => "terminal",
        }
    }
}

/// Desde dónde llega un cambio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Canal {
    Web,
    Terminal,
}

impl Canal {
    pub fn nombre(self) -> &'static str {
        match self {
            Canal::Web => "web",
            Canal::Terminal => "terminal",
        }
    }
}

/// Cuándo tiene efecto un cambio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aplica {
    EnCaliente,
    /// Hay que reiniciar estos servicios.
    Reinicio(&'static [&'static str]),
}

impl Aplica {
    pub fn nombre(self) -> &'static str {
        match self {
            Aplica::EnCaliente => "caliente",
            Aplica::Reinicio(_) => "reinicio",
        }
    }

    pub fn servicios(self) -> &'static [&'static str] {
        match self {
            Aplica::EnCaliente => &[],
            Aplica::Reinicio(s) => s,
        }
    }
}

#[derive(Debug)]
pub struct Clave {
    /// `seccion.subseccion.clave`, igual que en `ajustes.toml`. Con `*` es un
    /// patrón: `modelos.*.nombre` vale para cada perfil y `agentes.*.bin` para
    /// cada agente del catálogo (ver `concretas`).
    pub nombre: &'static str,
    pub seccion: &'static str,
    pub titulo: &'static str,
    pub descripcion: &'static str,
    pub tipo: Tipo,
    pub edicion: Edicion,
    /// Desde la web solo puede bajar respecto al valor efectivo.
    pub endurecible: bool,
    pub aplica: Aplica,
    /// Variable de entorno que la fija (y entonces nadie la cambia).
    pub entorno: Option<&'static str>,
    /// Fuentes antiguas, por orden de preferencia: fichero y ruta con puntos
    /// dentro de él. Las de una instancia de un patrón están en
    /// `HEREDADAS_INSTANCIA`.
    pub heredada: &'static [(&'static str, &'static str)],
}

/// Secciones en el orden en que se muestran.
pub static SECCIONES: &[(&str, &str)] = &[
    ("roles", "Roles"),
    ("agentes", "Agentes"),
    ("modelos", "Modelos"),
    ("agente", "Ejecutor: contención y skills"),
    ("orquestador", "Orquestador"),
    ("red", "Red"),
    ("rutas", "Rutas"),
    ("web", "Panel web"),
    ("biblioteca", "Biblioteca"),
];

const TODOS: &[&str] = &["ea-orchestrator", "ea-web", "ea-mcp"];
const ORQ: &str = "orquestador.toml";
/// La sección `[modelo]` que `ajustes.toml` tenía antes de la 017: se lee
/// como fuente heredada del perfil `local` y de `agentes.qwen-code.bin`.
pub(crate) const AJ_VIEJO: &str = "ajustes.toml [modelo]";
/// Claves antiguas de `ajustes.toml` y su nombre de ahora.
pub static ALIAS: &[(&str, &str)] = &[
    ("modelo.nombre", "modelos.local.nombre"),
    ("modelo.base_url", "modelos.local.base_url"),
    ("modelo.qwen_bin", "agentes.qwen-code.bin"),
];

/// Fuentes heredadas de instancias concretas de un patrón.
static HEREDADAS_INSTANCIA: &[(&str, &[(&str, &str)])] = &[
    ("modelos.local.nombre", &[(AJ_VIEJO, "modelo.nombre"), (ORQ, "modelo")]),
    ("modelos.local.base_url", &[(AJ_VIEJO, "modelo.base_url"), (ORQ, "base_url")]),
    ("agentes.qwen-code.bin", &[(AJ_VIEJO, "modelo.qwen_bin"), (ORQ, "qwen_bin")]),
];

pub static CLAVES: &[Clave] = &[
    Clave {
        nombre: "roles.planificador",
        seccion: "roles",
        titulo: "Planificador",
        descripcion: "Agente que formaliza y revisa (sesión interactiva). No puede ser el mismo que el ejecutor.",
        tipo: Tipo::Eleccion { permitidos: IDS_AGENTES },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "roles.ejecutor",
        seccion: "roles",
        titulo: "Ejecutor",
        descripcion: "Agente que ejecuta desatendido en bwrap con un modelo local. El cambio vale para el próximo trabajo que se tome; el que está en curso sigue con el suyo.",
        tipo: Tipo::Eleccion { permitidos: IDS_AGENTES },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "agentes.*.bin",
        seccion: "agentes",
        titulo: "binario",
        descripcion: "Programa que se lanza. Es código que se ejecuta: solo desde la terminal.",
        tipo: Tipo::Texto { max: 4096 },
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "agentes.*.modelo",
        seccion: "agentes",
        titulo: "modelo",
        descripcion: "Perfil de modelo (modelos.<id>) que usa el agente; vacío: el que tenga configurado él mismo (solo para un planificador). El del ejecutor tiene que ser local.",
        tipo: Tipo::Perfil,
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "modelos.*.nombre",
        seccion: "modelos",
        titulo: "nombre del modelo",
        descripcion: "Nombre del modelo tal como lo pide el agente (-m).",
        tipo: Tipo::Texto { max: 200 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "modelos.*.base_url",
        seccion: "modelos",
        titulo: "URL del servidor",
        descripcion: "Endpoint compatible con OpenAI. Un perfil local solo admite 127.0.0.1, localhost o [::1]; uno remoto puede dejarla vacía.",
        tipo: Tipo::UrlPerfil,
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "modelos.*.remoto",
        seccion: "modelos",
        titulo: "remoto",
        descripcion: "Modelo en la nube: solo lo puede usar el planificador, nunca el ejecutor.",
        tipo: Tipo::Booleano,
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "agente.sandbox",
        seccion: "agente",
        titulo: "Sandbox del sistema",
        descripcion: "Ejecuta al agente ejecutor dentro del sandbox del sistema (bwrap en Linux, Seatbelt en macOS; en Windows nativo no hay y el ejecutor se niega): solo el worktree es escribible. Qué sandbox usa esta máquina y si está disponible se ve en Sistema. Apagarlo solo desde la terminal.",
        tipo: Tipo::Booleano,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "sandbox")],
    },
    Clave {
        nombre: "agente.avisos_escritorio",
        seccion: "agente",
        titulo: "Avisos de escritorio",
        descripcion: "notify-send al terminar cada trabajo.",
        tipo: Tipo::Booleano,
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "avisos_escritorio")],
    },
    Clave {
        nombre: "agente.techo_topes.turnos",
        seccion: "agente",
        titulo: "Techo de turnos",
        descripcion: "Máximo de turnos del ejecutor por llamada (si su CLI lo admite); las órdenes que pidan más se recortan.",
        tipo: Tipo::Entero { min: 1, max: 100 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "topes.turnos")],
    },
    Clave {
        nombre: "agente.techo_topes.tiempo_s",
        seccion: "agente",
        titulo: "Techo de tiempo (s)",
        descripcion: "Máximo de segundos por llamada al ejecutor; las órdenes que pidan más se recortan.",
        tipo: Tipo::Entero { min: 60, max: 3600 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "topes.tiempo_s")],
    },
    Clave {
        nombre: "agente.techo_topes.llamadas_herramienta",
        seccion: "agente",
        titulo: "Techo de llamadas a herramientas",
        descripcion: "Máximo de llamadas a herramientas por llamada al ejecutor (si su CLI lo admite).",
        tipo: Tipo::Entero { min: 1, max: 200 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "topes.llamadas_herramienta")],
    },
    Clave {
        nombre: "agente.techo_topes.iteraciones",
        seccion: "agente",
        titulo: "Techo de iteraciones",
        descripcion: "Máximo de reintentos con evidencia por misión o por fase de obra.",
        tipo: Tipo::Entero { min: 1, max: 10 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "topes.iteraciones")],
    },
    Clave {
        nombre: "agente.skills.dir",
        seccion: "agente",
        titulo: "Catálogo de skills del ejecutor",
        descripcion: "Directorio con las skills <nombre>.md del ejecutor.",
        tipo: Tipo::Ruta,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "skills.dir")],
    },
    Clave {
        nombre: "agente.skills.habilitadas",
        seccion: "agente",
        titulo: "Skills habilitadas",
        descripcion: "Skills que se pueden usar; vacío: todas las del catálogo.",
        tipo: Tipo::Lista { permitidos: None },
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "skills.habilitadas")],
    },
    Clave {
        nombre: "agente.skills.max_por_trabajo",
        seccion: "agente",
        titulo: "Skills por trabajo",
        descripcion: "Máximo de skills en un trabajo o en una fase de obra.",
        tipo: Tipo::Entero { min: 0, max: 5 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "skills.max_por_trabajo")],
    },
    Clave {
        nombre: "agente.skills.max_caracteres",
        seccion: "agente",
        titulo: "Caracteres de skills",
        descripcion: "Máximo de caracteres que suman los cuerpos de las skills elegidas.",
        tipo: Tipo::Entero { min: 0, max: 12000 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "skills.max_caracteres")],
    },
    Clave {
        nombre: "agente.skills.pueden_elegir",
        seccion: "agente",
        titulo: "Quién elige skills",
        descripcion: "Quién tiene la llave para elegir skills: el rol planificador (planificador, o claude-code, su nombre en disco) o nadie.",
        tipo: Tipo::Lista { permitidos: Some(&["claude-code", "planificador"]) },
        edicion: Edicion::Terminal,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[(ORQ, "skills.pueden_elegir")],
    },
    Clave {
        nombre: "orquestador.intervalo_sondeo_s",
        seccion: "orquestador",
        titulo: "Sondeo de la cola (s)",
        descripcion: "Cada cuánto mira el orquestador la cola modelo-local cuando está vacía.",
        tipo: Tipo::Entero { min: 1, max: 300 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::Reinicio(&["ea-orchestrator"]),
        entorno: None,
        heredada: &[("orchestrator.toml", "intervalo_sondeo_s")],
    },
    Clave {
        nombre: "orquestador.paralelo",
        seccion: "orquestador",
        titulo: "Ejecutores en paralelo",
        descripcion: "Tope global de ejecutores trabajando a la vez (spec 019). 1: uno solo, como sin equipos. Más paralelismo es más trabajo desatendido a la vez, así que solo se sube desde la terminal.",
        tipo: Tipo::Entero { min: 1, max: 8 },
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::Reinicio(&["ea-orchestrator"]),
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "orquestador.intervalo_heartbeat_s",
        seccion: "orquestador",
        titulo: "Latido (s)",
        descripcion: "Cada cuánto apunta el orquestador su latido; se da por vivo hasta el doble.",
        tipo: Tipo::Entero { min: 5, max: 600 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::Reinicio(TODOS),
        entorno: None,
        heredada: &[("orchestrator.toml", "intervalo_heartbeat_s")],
    },
    Clave {
        nombre: "orquestador.rutinas_dir",
        seccion: "orquestador",
        titulo: "Directorio de rutinas",
        descripcion: "Dónde están los ficheros TOML de las rutinas.",
        tipo: Tipo::Ruta,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::Reinicio(&["ea-web", "ea-mcp"]),
        entorno: None,
        heredada: &[("orchestrator.toml", "rutinas_dir")],
    },
    Clave {
        nombre: "red.puerto_web",
        seccion: "red",
        titulo: "Puerto del panel web",
        descripcion: "ea-web escucha en 127.0.0.1 y este puerto (el host no se configura).",
        tipo: Tipo::Entero { min: 1024, max: 65535 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::Reinicio(&["ea-web"]),
        entorno: Some("EA_WEB_PORT"),
        heredada: &[],
    },
    Clave {
        nombre: "red.puerto_mcp",
        seccion: "red",
        titulo: "Puerto del servidor MCP",
        descripcion: "ea-mcp escucha en 127.0.0.1 y este puerto. Al cambiarlo, cambia también la URL del MCP en Claude Code.",
        tipo: Tipo::Entero { min: 1024, max: 65535 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::Reinicio(&["ea-mcp"]),
        entorno: Some("EA_MCP_PORT"),
        heredada: &[],
    },
    Clave {
        nombre: "rutas.db",
        seccion: "rutas",
        titulo: "Base de datos",
        descripcion: "Fichero SQLite del diario, las propuestas y el índice de la bóveda.",
        tipo: Tipo::Ruta,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::Reinicio(TODOS),
        entorno: Some("EA_DB_PATH"),
        heredada: &[],
    },
    Clave {
        nombre: "rutas.estado",
        seccion: "rutas",
        titulo: "Estado del bus",
        descripcion: "Colas, trabajos y semáforos. El hook de subagentes solo lee EA_ESTADO_DIR, no este fichero.",
        tipo: Tipo::Ruta,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::Reinicio(TODOS),
        entorno: Some("EA_ESTADO_DIR"),
        heredada: &[],
    },
    Clave {
        nombre: "rutas.boveda",
        seccion: "rutas",
        titulo: "Bóveda de notas",
        descripcion: "Raíz de la bóveda (con sus regiones).",
        tipo: Tipo::Ruta,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::Reinicio(&["ea-web", "ea-mcp"]),
        entorno: Some("EA_BOVEDA_DIR"),
        heredada: &[("boveda.toml", "raiz")],
    },
    Clave {
        nombre: "web.sondeo_ms",
        seccion: "web",
        titulo: "Refresco de la página (ms)",
        descripcion: "Cada cuánto se actualiza la sección abierta del panel.",
        tipo: Tipo::Entero { min: 1000, max: 60000 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "biblioteca.categorias",
        seccion: "biblioteca",
        titulo: "Categorías propias",
        descripcion: "Categoría que agrupa a cada elemento de la biblioteca: id del elemento (agente/tipo/ámbito/nombre) a nombre de categoría. Sin entrada, la categoría es el origen.",
        tipo: Tipo::Mapa,
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
    Clave {
        nombre: "web.diario_limite",
        seccion: "web",
        titulo: "Eventos del diario",
        descripcion: "Cuántos eventos carga la sección Diario.",
        tipo: Tipo::Entero { min: 10, max: 1000 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: &[],
    },
];

/// La definición de una clave concreta: una fija o la instancia de un patrón
/// (`modelos.<id>.campo` con un id válido, `agentes.<id>.campo` con un agente
/// del catálogo).
pub fn clave(nombre: &str) -> Option<&'static Clave> {
    if let Some(c) = CLAVES.iter().find(|c| c.nombre == nombre && !c.nombre.contains('*')) {
        return Some(c);
    }
    let (grupo, id, campo) = trocear(nombre)?;
    let valido = match grupo {
        "modelos" => id_perfil_valido(id),
        "agentes" => ficha(id).is_some(),
        _ => false,
    };
    if !valido {
        return None;
    }
    CLAVES.iter().find(|c| c.nombre.strip_prefix(grupo).and_then(|r| r.strip_prefix(".*.")) == Some(campo))
}

/// `grupo.id.campo` en sus tres trozos.
fn trocear(nombre: &str) -> Option<(&str, &str, &str)> {
    let mut p = nombre.splitn(3, '.');
    let (g, id, campo) = (p.next()?, p.next()?, p.next()?);
    (!campo.contains('.')).then_some((g, id, campo))
}

/// El nombre de ahora de una clave antigua de `ajustes.toml` (`modelo.*`).
pub fn sin_alias(nombre: &str) -> &str {
    ALIAS.iter().find(|(viejo, _)| *viejo == nombre).map_or(nombre, |(_, nuevo)| nuevo)
}

/// Las fuentes heredadas de una clave concreta.
pub(crate) fn heredadas(nombre: &str, c: &'static Clave) -> &'static [(&'static str, &'static str)] {
    HEREDADAS_INSTANCIA.iter().find(|(n, _)| *n == nombre).map_or(c.heredada, |(_, h)| h)
}

/// Las claves concretas, en el orden en que se muestran: las fijas tal cual,
/// `agentes.*` para cada agente del catálogo y `modelos.*` para cada perfil
/// de `perfiles`.
pub fn concretas(perfiles: &[String]) -> Vec<(String, &'static Clave)> {
    let mut salida = Vec::new();
    let mut hechos: Vec<&str> = Vec::new();
    for c in CLAVES {
        let Some((grupo, _)) = c.nombre.split_once(".*.") else {
            salida.push((c.nombre.to_string(), c));
            continue;
        };
        if hechos.contains(&grupo) {
            continue;
        }
        hechos.push(grupo);
        let ids: Vec<&str> = match grupo {
            "agentes" => IDS_AGENTES.to_vec(),
            _ => perfiles.iter().map(String::as_str).collect(),
        };
        let patrones: Vec<&'static Clave> =
            CLAVES.iter().filter(|p| p.nombre.split_once(".*.").is_some_and(|(g, _)| g == grupo)).collect();
        for id in ids {
            for p in &patrones {
                let campo = p.nombre.rsplit('.').next().unwrap_or_default();
                salida.push((format!("{grupo}.{id}.{campo}"), *p));
            }
        }
    }
    salida
}

/// Título para la interfaz: el de la clave o, en un patrón, con el agente o
/// el perfil delante («Codex · binario», «local · remoto»).
pub fn titulo(nombre: &str, c: &Clave) -> String {
    if !c.nombre.contains('*') {
        return c.titulo.to_string();
    }
    let (grupo, id, _) = trocear(nombre).unwrap_or_default();
    let quien = if grupo == "agentes" { ficha(id).map_or(id, |f| f.nombre) } else { id };
    format!("{quien} · {}", c.titulo)
}

fn ruta(p: &std::path::Path) -> Value {
    Value::String(p.to_string_lossy().into_owned())
}

/// Valor de una clave en unos ajustes tipados.
pub(crate) fn valor_de(a: &Ajustes, nombre: &str) -> Value {
    if let Some((grupo, id, campo)) = trocear(nombre) {
        match (grupo, campo) {
            ("modelos", _) => {
                return a.modelos.get(id).map_or(Value::Null, |p| match campo {
                    "nombre" => p.nombre.clone().into(),
                    "base_url" => p.base_url.clone().into(),
                    "remoto" => p.remoto.into(),
                    _ => Value::Null,
                });
            }
            ("agentes", _) => {
                return a.agentes.get(id).map_or(Value::Null, |g| match campo {
                    "bin" => g.bin.clone().into(),
                    "modelo" => g.modelo.clone().into(),
                    _ => Value::Null,
                });
            }
            _ => {}
        }
    }
    match nombre {
        "roles.planificador" => a.roles.planificador.clone().into(),
        "roles.ejecutor" => a.roles.ejecutor.clone().into(),
        "agente.sandbox" => a.agente.sandbox.into(),
        "agente.avisos_escritorio" => a.agente.avisos_escritorio.into(),
        "agente.techo_topes.turnos" => a.agente.techo_topes.turnos.into(),
        "agente.techo_topes.tiempo_s" => a.agente.techo_topes.tiempo_s.into(),
        "agente.techo_topes.llamadas_herramienta" => a.agente.techo_topes.llamadas_herramienta.into(),
        "agente.techo_topes.iteraciones" => a.agente.techo_topes.iteraciones.into(),
        "agente.skills.dir" => a.agente.skills.dir.clone().into(),
        "agente.skills.habilitadas" => a.agente.skills.habilitadas.clone().into(),
        "agente.skills.max_por_trabajo" => a.agente.skills.max_por_trabajo.into(),
        "agente.skills.max_caracteres" => a.agente.skills.max_caracteres.into(),
        "agente.skills.pueden_elegir" => a.agente.skills.pueden_elegir.clone().into(),
        "orquestador.intervalo_sondeo_s" => a.orquestador.intervalo_sondeo_s.into(),
        "orquestador.intervalo_heartbeat_s" => a.orquestador.intervalo_heartbeat_s.into(),
        "orquestador.paralelo" => a.orquestador.paralelo.into(),
        "orquestador.rutinas_dir" => ruta(&a.orquestador.rutinas_dir),
        "red.puerto_web" => a.red.puerto_web.into(),
        "red.puerto_mcp" => a.red.puerto_mcp.into(),
        "rutas.db" => ruta(&a.rutas.db),
        "rutas.estado" => ruta(&a.rutas.estado),
        "rutas.boveda" => ruta(&a.rutas.boveda),
        "web.sondeo_ms" => a.web.sondeo_ms.into(),
        "web.diario_limite" => a.web.diario_limite.into(),
        "biblioteca.categorias" => serde_json::to_value(&a.biblioteca.categorias).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn lista(v: &Value) -> Vec<String> {
    v.as_array().map(|l| l.iter().filter_map(|e| e.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// Pone un valor YA VALIDADO en unos ajustes tipados (las rutas se expanden).
pub(crate) fn aplicar(a: &mut Ajustes, nombre: &str, v: &Value) {
    let n = || v.as_i64().unwrap_or_default();
    let t = || v.as_str().unwrap_or_default().to_string();
    let r = || expandir_tilde(v.as_str().unwrap_or_default());
    let b = || v.as_bool().unwrap_or_default();
    if let Some((grupo, id, campo)) = trocear(nombre) {
        match grupo {
            "modelos" => {
                let p = a.modelos.entry(id.to_string()).or_insert_with(|| PerfilModelo {
                    nombre: String::new(),
                    base_url: String::new(),
                    remoto: false,
                });
                match campo {
                    "nombre" => p.nombre = t(),
                    "base_url" => p.base_url = t(),
                    "remoto" => p.remoto = b(),
                    _ => {}
                }
                return;
            }
            "agentes" => {
                let g = a.agentes.entry(id.to_string()).or_insert_with(|| ConfigAgente { bin: id.to_string(), modelo: String::new() });
                match campo {
                    "bin" => g.bin = t(),
                    "modelo" => g.modelo = t(),
                    _ => {}
                }
                return;
            }
            _ => {}
        }
    }
    match nombre {
        "roles.planificador" => a.roles.planificador = t(),
        "roles.ejecutor" => a.roles.ejecutor = t(),
        "agente.sandbox" => a.agente.sandbox = b(),
        "agente.avisos_escritorio" => a.agente.avisos_escritorio = b(),
        "agente.techo_topes.turnos" => a.agente.techo_topes.turnos = n() as u32,
        "agente.techo_topes.tiempo_s" => a.agente.techo_topes.tiempo_s = n() as u64,
        "agente.techo_topes.llamadas_herramienta" => a.agente.techo_topes.llamadas_herramienta = n() as u32,
        "agente.techo_topes.iteraciones" => a.agente.techo_topes.iteraciones = n() as u32,
        "agente.skills.dir" => a.agente.skills.dir = r().to_string_lossy().into_owned(),
        "agente.skills.habilitadas" => a.agente.skills.habilitadas = lista(v),
        "agente.skills.max_por_trabajo" => a.agente.skills.max_por_trabajo = n() as usize,
        "agente.skills.max_caracteres" => a.agente.skills.max_caracteres = n() as usize,
        "agente.skills.pueden_elegir" => a.agente.skills.pueden_elegir = lista(v),
        "orquestador.intervalo_sondeo_s" => a.orquestador.intervalo_sondeo_s = n() as u64,
        "orquestador.intervalo_heartbeat_s" => a.orquestador.intervalo_heartbeat_s = n() as u64,
        "orquestador.paralelo" => a.orquestador.paralelo = n() as u32,
        "orquestador.rutinas_dir" => a.orquestador.rutinas_dir = r(),
        "red.puerto_web" => a.red.puerto_web = n() as u16,
        "red.puerto_mcp" => a.red.puerto_mcp = n() as u16,
        "rutas.db" => a.rutas.db = r(),
        "rutas.estado" => a.rutas.estado = r(),
        "rutas.boveda" => a.rutas.boveda = r(),
        "web.sondeo_ms" => a.web.sondeo_ms = n() as u64,
        "web.diario_limite" => a.web.diario_limite = n() as u32,
        "biblioteca.categorias" => {
            a.biblioteca.categorias = v
                .as_object()
                .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
                .unwrap_or_default();
        }
        _ => {}
    }
}
