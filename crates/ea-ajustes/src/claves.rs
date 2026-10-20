//! Registro único de claves: la API, la CLI y la página salen de aquí. Cada
//! clave dice su tipo y rango, quién la puede cambiar, cuándo se aplica, qué
//! variable de entorno la fija y de qué fichero antiguo se hereda.

use serde_json::Value;

use crate::fuentes::Ajustes;
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
    /// `seccion.subseccion.clave`, igual que en `ajustes.toml`.
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
    /// Fichero antiguo y ruta con puntos dentro de él.
    pub heredada: Option<(&'static str, &'static str)>,
}

/// Secciones en el orden en que se muestran.
pub static SECCIONES: &[(&str, &str)] = &[
    ("modelo", "Modelo local"),
    ("agente", "Agente local"),
    ("orquestador", "Orquestador"),
    ("red", "Red"),
    ("rutas", "Rutas"),
    ("web", "Panel web"),
];

const TODOS: &[&str] = &["ea-orchestrator", "ea-web", "ea-mcp"];
const ORQ: &str = "orquestador.toml";

pub static CLAVES: &[Clave] = &[
    Clave {
        nombre: "modelo.nombre",
        seccion: "modelo",
        titulo: "Modelo",
        descripcion: "Nombre del modelo que sirve llama-server y usa Qwen Code (-m).",
        tipo: Tipo::Texto { max: 200 },
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "modelo")),
    },
    Clave {
        nombre: "modelo.base_url",
        seccion: "modelo",
        titulo: "URL del servidor",
        descripcion: "Endpoint OpenAI de llama-server. Solo 127.0.0.1, localhost o [::1].",
        tipo: Tipo::Url,
        edicion: Edicion::Web,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "base_url")),
    },
    Clave {
        nombre: "modelo.qwen_bin",
        seccion: "modelo",
        titulo: "Binario de Qwen Code",
        descripcion: "Programa que lanza el ejecutor. Es código que se ejecuta: solo desde la terminal.",
        tipo: Tipo::Texto { max: 4096 },
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "qwen_bin")),
    },
    Clave {
        nombre: "agente.sandbox",
        seccion: "agente",
        titulo: "Sandbox (bubblewrap)",
        descripcion: "Ejecuta Qwen dentro de bwrap: solo el worktree es escribible. Apagarlo solo desde la terminal.",
        tipo: Tipo::Booleano,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "sandbox")),
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
        heredada: Some((ORQ, "avisos_escritorio")),
    },
    Clave {
        nombre: "agente.techo_topes.turnos",
        seccion: "agente",
        titulo: "Techo de turnos",
        descripcion: "Máximo de turnos de Qwen por llamada; las órdenes que pidan más se recortan.",
        tipo: Tipo::Entero { min: 1, max: 100 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "topes.turnos")),
    },
    Clave {
        nombre: "agente.techo_topes.tiempo_s",
        seccion: "agente",
        titulo: "Techo de tiempo (s)",
        descripcion: "Máximo de segundos por llamada a Qwen; las órdenes que pidan más se recortan.",
        tipo: Tipo::Entero { min: 60, max: 3600 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "topes.tiempo_s")),
    },
    Clave {
        nombre: "agente.techo_topes.llamadas_herramienta",
        seccion: "agente",
        titulo: "Techo de llamadas a herramientas",
        descripcion: "Máximo de llamadas a herramientas por llamada a Qwen.",
        tipo: Tipo::Entero { min: 1, max: 200 },
        edicion: Edicion::Web,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "topes.llamadas_herramienta")),
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
        heredada: Some((ORQ, "topes.iteraciones")),
    },
    Clave {
        nombre: "agente.skills.dir",
        seccion: "agente",
        titulo: "Catálogo de skills de Qwen",
        descripcion: "Directorio con las skills <nombre>.md del agente local.",
        tipo: Tipo::Ruta,
        edicion: Edicion::Terminal,
        endurecible: false,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "skills.dir")),
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
        heredada: Some((ORQ, "skills.habilitadas")),
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
        heredada: Some((ORQ, "skills.max_por_trabajo")),
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
        heredada: Some((ORQ, "skills.max_caracteres")),
    },
    Clave {
        nombre: "agente.skills.pueden_elegir",
        seccion: "agente",
        titulo: "Quién elige skills",
        descripcion: "Valores de creada_por que tienen la llave para elegir skills (solo claude-code o nadie).",
        tipo: Tipo::Lista { permitidos: Some(&["claude-code"]) },
        edicion: Edicion::Terminal,
        endurecible: true,
        aplica: Aplica::EnCaliente,
        entorno: None,
        heredada: Some((ORQ, "skills.pueden_elegir")),
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
        heredada: Some(("orchestrator.toml", "intervalo_sondeo_s")),
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
        heredada: Some(("orchestrator.toml", "intervalo_heartbeat_s")),
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
        heredada: Some(("orchestrator.toml", "rutinas_dir")),
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
        heredada: None,
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
        heredada: None,
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
        heredada: None,
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
        heredada: None,
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
        heredada: Some(("boveda.toml", "raiz")),
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
        heredada: None,
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
        heredada: None,
    },
];

pub fn clave(nombre: &str) -> Option<&'static Clave> {
    CLAVES.iter().find(|c| c.nombre == nombre)
}

fn ruta(p: &std::path::Path) -> Value {
    Value::String(p.to_string_lossy().into_owned())
}

/// Valor de una clave en unos ajustes tipados.
pub(crate) fn valor_de(a: &Ajustes, nombre: &str) -> Value {
    match nombre {
        "modelo.nombre" => a.modelo.nombre.clone().into(),
        "modelo.base_url" => a.modelo.base_url.clone().into(),
        "modelo.qwen_bin" => a.modelo.qwen_bin.clone().into(),
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
        "orquestador.rutinas_dir" => ruta(&a.orquestador.rutinas_dir),
        "red.puerto_web" => a.red.puerto_web.into(),
        "red.puerto_mcp" => a.red.puerto_mcp.into(),
        "rutas.db" => ruta(&a.rutas.db),
        "rutas.estado" => ruta(&a.rutas.estado),
        "rutas.boveda" => ruta(&a.rutas.boveda),
        "web.sondeo_ms" => a.web.sondeo_ms.into(),
        "web.diario_limite" => a.web.diario_limite.into(),
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
    match nombre {
        "modelo.nombre" => a.modelo.nombre = t(),
        "modelo.base_url" => a.modelo.base_url = t(),
        "modelo.qwen_bin" => a.modelo.qwen_bin = t(),
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
        "orquestador.rutinas_dir" => a.orquestador.rutinas_dir = r(),
        "red.puerto_web" => a.red.puerto_web = n() as u16,
        "red.puerto_mcp" => a.red.puerto_mcp = n() as u16,
        "rutas.db" => a.rutas.db = r(),
        "rutas.estado" => a.rutas.estado = r(),
        "rutas.boveda" => a.rutas.boveda = r(),
        "web.sondeo_ms" => a.web.sondeo_ms = n() as u64,
        "web.diario_limite" => a.web.diario_limite = n() as u32,
        _ => {}
    }
}
