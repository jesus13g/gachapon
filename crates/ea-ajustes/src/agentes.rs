//! Roles y catálogo cerrado de agentes (spec 017, constitución 1.3.0).
//!
//! Un **rol** es lo que hace un agente en el sistema: el planificador
//! formaliza y revisa; el ejecutor ejecuta desatendido. En disco (colas,
//! regiones, `creada_por`) cada rol conserva su nombre de siempre
//! (`claude-code` y `modelo-local`), así que no hay migración.
//!
//! El catálogo de agentes vive en el código porque cada agente necesita su
//! adaptador; `ajustes.toml` solo dice qué agente ocupa cada rol, con qué
//! binario y con qué perfil de modelo. Cada agente es un [`AgenteId`] cerrado
//! y su [`FichaAgente`] reúne lo estático (binario por defecto, carpeta,
//! familia visual, sesión, adaptador de ejecutor); en disco, en la API y en
//! los mensajes sigue yendo su id de texto.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rol {
    #[serde(rename = "claude-code", alias = "planificador")]
    Planificador,
    #[serde(rename = "modelo-local", alias = "ejecutor")]
    Ejecutor,
}

impl Rol {
    pub const TODOS: [Rol; 2] = [Rol::Planificador, Rol::Ejecutor];

    /// Nombre del rol en la interfaz y en `ajustes.toml` (`roles.<nombre>`).
    pub fn nombre(self) -> &'static str {
        match self {
            Rol::Planificador => "planificador",
            Rol::Ejecutor => "ejecutor",
        }
    }

    /// Nombre en disco: cola, región, `creada_por` y actor.
    pub const fn as_str(self) -> &'static str {
        match self {
            Rol::Planificador => "claude-code",
            Rol::Ejecutor => "modelo-local",
        }
    }

    /// Acepta el nombre del rol y el de disco.
    pub fn desde(texto: &str) -> Option<Rol> {
        match texto.trim() {
            "planificador" | "claude-code" => Some(Rol::Planificador),
            "ejecutor" | "modelo-local" => Some(Rol::Ejecutor),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ actores

/// Los actores que no son un rol (los de los roles son [`Rol::as_str`]):
/// quien firma en el diario y de quien salen los permisos.
///
/// El usuario, por sus interfaces (la CLI y la web): el único que acepta
/// propuestas y edita la bóveda a mano.
pub const ACTOR_USUARIO: &str = "usuario";
/// El orquestador, desatendido (§11.2): solo lee.
pub const ACTOR_ORQUESTADOR: &str = "orquestador";
/// Un agente del catálogo que no ocupa ningún rol: solo lee.
pub const ACTOR_SIN_ROL: &str = "sin-rol";
/// Un cliente MCP que no es ningún agente del catálogo: solo lee.
pub const ACTOR_DESCONOCIDO: &str = "desconocido";

/// Un agente del catálogo. Un `match` sobre él es exhaustivo: añadir uno
/// obliga a decidir qué hace en cada sitio. Fuera del código va su id de
/// texto (`as_str`), el de siempre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AgenteId {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "qwen-code")]
    QwenCode,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "antigravity")]
    Antigravity,
}

impl AgenteId {
    /// En el orden en que se muestran (el de [`CATALOGO`]).
    pub const TODOS: [AgenteId; 5] = [Self::ClaudeCode, Self::QwenCode, Self::Codex, Self::OpenCode, Self::Antigravity];

    /// El id en disco, en la API y en los mensajes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::QwenCode => "qwen-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Antigravity => "antigravity",
        }
    }

    /// El agente con ese id, tal cual (sin recortar ni cambiar mayúsculas).
    pub fn desde(id: &str) -> Option<Self> {
        Self::TODOS.into_iter().find(|a| a.as_str() == id)
    }

    pub const fn ficha(self) -> &'static FichaAgente {
        &CATALOGO[self as usize]
    }
}

/// El adaptador de cada agente que puede ejecutar (spec 017): cada CLI tiene
/// sus argumentos y su formato de salida. Es el `ejecutor` de su ficha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoAgente {
    QwenCode,
    Codex,
    OpenCode,
}

impl TipoAgente {
    /// El tipo de un agente del catálogo que admite el rol de ejecutor.
    pub fn desde(id: &str) -> Option<Self> {
        AgenteId::desde(id)?.ficha().ejecutor
    }

    pub const fn agente(self) -> AgenteId {
        match self {
            Self::QwenCode => AgenteId::QwenCode,
            Self::Codex => AgenteId::Codex,
            Self::OpenCode => AgenteId::OpenCode,
        }
    }

    pub const fn id(self) -> &'static str {
        self.agente().as_str()
    }
}

/// Lo estático de un agente del catálogo.
#[derive(Debug)]
pub struct FichaAgente {
    pub id: AgenteId,
    /// Nombre para la interfaz.
    pub nombre: &'static str,
    /// Roles que puede ocupar.
    pub roles: &'static [Rol],
    /// Su adaptador si puede ejecutar (si admite el rol de ejecutor).
    pub ejecutor: Option<TipoAgente>,
    /// El binario de `agentes.<id>.bin` por defecto.
    pub bin_por_defecto: &'static str,
    /// Su carpeta de configuración bajo el HOME, con `/`.
    pub dir_config: &'static str,
    /// La variante de su máquina en la escena del panel (`VARIANTES` en
    /// `gashapon.js`).
    pub familia: &'static str,
    /// Admite una sesión con solo las herramientas elegidas (biblioteca,
    /// spec 018).
    pub admite_sesion: bool,
    /// Qué topes propios admite su CLI (además del `timeout` del sistema).
    pub topes: &'static str,
    /// Nombres con que se presenta como cliente MCP (`clientInfo.name` del
    /// `initialize`), comprobados con cada CLI (spec 017, T-009).
    pub clientes_mcp: &'static [&'static str],
    /// La opción que pasa el prompt inicial a una sesión interactiva; `None`:
    /// el prompt va solo, tras el binario.
    pub opcion_prompt: Option<&'static str>,
}

impl FichaAgente {
    pub fn admite(&self, rol: Rol) -> bool {
        self.roles.contains(&rol)
    }

    /// El argv que abre una sesión del agente con un prompt inicial;
    /// interactivo (el prompt no cierra la sesión). Sin herramientas ni
    /// opciones extra: es la configuración habitual del agente.
    pub fn argv_interactivo(&self, bin: &str, prompt: &str) -> Vec<String> {
        let mut argv = vec![bin.to_string()];
        argv.extend(self.opcion_prompt.map(String::from));
        argv.push(prompt.to_string());
        argv
    }
}

const AMBOS: &[Rol] = &[Rol::Planificador, Rol::Ejecutor];
const PLANIFICADOR: &[Rol] = &[Rol::Planificador];

/// Los ids de las fichas, todas o solo las que admiten sesión, en el orden
/// del catálogo. `N` es cuántas son: si no casa, no compila.
const fn ids<const N: usize>(solo_con_sesion: bool) -> [&'static str; N] {
    let mut ids = [""; N];
    let (mut i, mut n) = (0, 0);
    while i < CATALOGO.len() {
        if !solo_con_sesion || CATALOGO[i].admite_sesion {
            ids[n] = CATALOGO[i].id.as_str();
            n += 1;
        }
        i += 1;
    }
    assert!(n == N, "N no es el número de agentes");
    ids
}

/// Ids del catálogo, en el orden en que se muestran.
pub const IDS_AGENTES: &[&str] = &ids::<{ AgenteId::TODOS.len() }>(false);

/// Los que admiten sesión ([`FichaAgente::admite_sesion`]), en el mismo orden.
pub const IDS_CON_SESION: [&str; 4] = ids(true);

/// Una ficha por agente, en el orden de [`AgenteId::TODOS`] (lo usa
/// [`AgenteId::ficha`]).
pub static CATALOGO: &[FichaAgente] = &[
    FichaAgente {
        id: AgenteId::ClaudeCode,
        nombre: "Claude Code",
        roles: PLANIFICADOR,
        ejecutor: None,
        bin_por_defecto: "claude",
        dir_config: ".claude",
        familia: "claude",
        admite_sesion: true,
        topes: "—",
        clientes_mcp: &["claude-code"],
        opcion_prompt: None,
    },
    FichaAgente {
        id: AgenteId::QwenCode,
        nombre: "Qwen Code",
        roles: AMBOS,
        ejecutor: Some(TipoAgente::QwenCode),
        bin_por_defecto: "qwen",
        dir_config: ".qwen",
        familia: "qwen",
        admite_sesion: true,
        topes: "turnos, llamadas y tiempo",
        clientes_mcp: &["qwen-code-mcp-client", "qwen-code"],
        opcion_prompt: Some("-i"),
    },
    FichaAgente {
        id: AgenteId::Codex,
        nombre: "Codex",
        roles: AMBOS,
        ejecutor: Some(TipoAgente::Codex),
        bin_por_defecto: "codex",
        dir_config: ".codex",
        familia: "codex",
        admite_sesion: true,
        topes: "solo tiempo",
        clientes_mcp: &["codex-mcp-client"],
        opcion_prompt: None,
    },
    FichaAgente {
        id: AgenteId::OpenCode,
        nombre: "OpenCode",
        roles: AMBOS,
        ejecutor: Some(TipoAgente::OpenCode),
        bin_por_defecto: "opencode",
        dir_config: ".config/opencode",
        familia: "opencode",
        admite_sesion: true,
        topes: "solo tiempo",
        clientes_mcp: &["opencode"],
        opcion_prompt: Some("--prompt"),
    },
    // `agy` 1.4 se presenta como `antigravity-client` (capturado del `initialize`).
    FichaAgente {
        id: AgenteId::Antigravity,
        nombre: "Antigravity",
        roles: PLANIFICADOR,
        ejecutor: None,
        bin_por_defecto: "agy",
        dir_config: ".gemini",
        familia: "antigravity",
        admite_sesion: false,
        topes: "—",
        clientes_mcp: &["antigravity-client"],
        opcion_prompt: Some("-i"),
    },
];

/// La ficha del agente con ese id de texto, si es del catálogo.
pub fn ficha(id: &str) -> Option<&'static FichaAgente> {
    AgenteId::desde(id).map(AgenteId::ficha)
}

/// El agente del catálogo que se presenta con ese nombre de cliente MCP.
pub fn por_cliente_mcp(nombre: &str) -> Option<&'static FichaAgente> {
    CATALOGO.iter().find(|f| f.clientes_mcp.contains(&nombre))
}

/// Id válido de un perfil de modelo: minúsculas, dígitos, `-` y `_`, hasta
/// 32 caracteres (es un trozo de clave de `ajustes.toml`).
pub fn id_perfil_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}

/// La ruta del binario: tal cual si lleva directorio, si no, en el `PATH`
/// dado (en Windows, con las extensiones de `PATHEXT`).
pub fn buscar_binario(bin: &str, path: Option<&std::ffi::OsStr>) -> Option<std::path::PathBuf> {
    ea_plataforma::rutas::buscar_binario(bin, path, std::env::var_os("PATHEXT").as_deref())
}

/// Un agente del catálogo tal como lo enseñan `ea agentes`, la TUI y
/// `GET /api/agentes`: qué rol ocupa, con qué binario y con qué modelo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VistaAgente {
    pub id: &'static str,
    pub nombre: &'static str,
    /// Roles que puede ocupar (`planificador`, `ejecutor`).
    pub admite: Vec<&'static str>,
    /// El rol que ocupa ahora, si ocupa alguno.
    pub rol: Option<&'static str>,
    pub bin: String,
    /// Dónde está el binario; `None`: no se encuentra.
    pub ruta: Option<String>,
    /// Id del perfil (`modelos.<id>`); vacío: el modelo propio del agente.
    pub perfil: String,
    pub modelo: Option<crate::PerfilModelo>,
    pub topes: &'static str,
}

impl crate::Ajustes {
    /// El catálogo con los ajustes efectivos; el binario se busca en `path`.
    pub fn vista_agentes(&self, path: Option<&std::ffi::OsStr>) -> Vec<VistaAgente> {
        CATALOGO
            .iter()
            .map(|f| {
                let id = f.id.as_str();
                let config = &self.agentes[id];
                VistaAgente {
                    id,
                    nombre: f.nombre,
                    admite: f.roles.iter().map(|r| r.nombre()).collect(),
                    rol: self.rol_de(id).map(Rol::nombre),
                    bin: config.bin.clone(),
                    ruta: buscar_binario(&config.bin, path).map(|p| p.display().to_string()),
                    perfil: config.modelo.clone(),
                    modelo: self.modelo_de(id).cloned(),
                    topes: f.topes,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_argv_de_cada_agente_lleva_el_prompt_interactivo() {
        let argv = |id: &str, bin: &str| ficha(id).unwrap().argv_interactivo(bin, "hola");
        assert_eq!(argv("claude-code", "claude"), ["claude", "hola"]);
        assert_eq!(argv("qwen-code", "qwen"), ["qwen", "-i", "hola"]);
        assert_eq!(argv("opencode", "opencode"), ["opencode", "--prompt", "hola"]);
        assert_eq!(argv("antigravity", "agy"), ["agy", "-i", "hola"]);
        assert_eq!(argv("codex", "codex"), ["codex", "hola"]);
    }

    #[test]
    fn cada_id_con_su_ficha_y_su_nombre_de_siempre() {
        let ids = AgenteId::TODOS.map(AgenteId::as_str);
        assert_eq!(ids, ["claude-code", "qwen-code", "codex", "opencode", "antigravity"]);
        assert_eq!(CATALOGO.iter().map(|f| f.id).collect::<Vec<_>>(), AgenteId::TODOS, "fichas en el orden del enum");
        for a in AgenteId::TODOS {
            assert_eq!((AgenteId::desde(a.as_str()), a.ficha().id), (Some(a), a));
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(a.as_str()));
            assert_eq!(serde_json::from_value::<AgenteId>(serde_json::json!(a.as_str())).unwrap(), a);
        }
        for otro in ["gpt", "", " codex", "Codex", "planificador", "modelo-local"] {
            assert_eq!(AgenteId::desde(otro), None, "{otro:?}");
            assert!(ficha(otro).is_none(), "{otro:?}");
            assert!(serde_json::from_value::<AgenteId>(serde_json::json!(otro)).is_err(), "{otro:?}");
        }
    }

    #[test]
    fn las_tablas_que_salen_de_las_fichas() {
        let de = |f: fn(&FichaAgente) -> &'static str| CATALOGO.iter().map(f).collect::<Vec<_>>();
        assert_eq!(de(|f| f.bin_por_defecto), ["claude", "qwen", "codex", "opencode", "agy"]);
        assert_eq!(de(|f| f.dir_config), [".claude", ".qwen", ".codex", ".config/opencode", ".gemini"]);
        assert_eq!(de(|f| f.familia), ["claude", "qwen", "codex", "opencode", "antigravity"]);
        assert_eq!(IDS_CON_SESION, ["claude-code", "qwen-code", "codex", "opencode"]);
        let tipos = CATALOGO.iter().map(|f| f.ejecutor).collect::<Vec<_>>();
        assert_eq!(tipos, [None, Some(TipoAgente::QwenCode), Some(TipoAgente::Codex), Some(TipoAgente::OpenCode), None]);
        for f in CATALOGO {
            // El adaptador de ejecutor es el del propio agente, y solo lo tiene quien puede ejecutar.
            assert_eq!(f.ejecutor.is_some(), f.admite(Rol::Ejecutor), "{:?}", f.id);
            assert!(f.ejecutor.is_none_or(|t| t.agente() == f.id), "{:?}", f.id);
        }
    }
}
