//! Roles y catálogo cerrado de agentes (spec 017, constitución 1.3.0).
//!
//! Un **rol** es lo que hace un agente en el sistema: el planificador
//! formaliza y revisa; el ejecutor ejecuta desatendido. En disco (colas,
//! regiones, `creada_por`) cada rol conserva su nombre de siempre
//! (`claude-code` y `modelo-local`), así que no hay migración.
//!
//! El catálogo de agentes vive en el código porque cada agente necesita su
//! adaptador; `ajustes.toml` solo dice qué agente ocupa cada rol, con qué
//! binario y con qué perfil de modelo.

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

/// Un agente del catálogo.
#[derive(Debug)]
pub struct FichaAgente {
    pub id: &'static str,
    /// Nombre para la interfaz.
    pub nombre: &'static str,
    /// Roles que puede ocupar.
    pub roles: &'static [Rol],
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

/// Ids del catálogo, en el orden en que se muestran.
pub const IDS_AGENTES: &[&str] = &["claude-code", "qwen-code", "codex", "opencode", "antigravity"];

pub static CATALOGO: &[FichaAgente] = &[
    FichaAgente {
        id: "claude-code",
        nombre: "Claude Code",
        roles: PLANIFICADOR,
        topes: "—",
        clientes_mcp: &["claude-code"],
        opcion_prompt: None,
    },
    FichaAgente {
        id: "qwen-code",
        nombre: "Qwen Code",
        roles: AMBOS,
        topes: "turnos, llamadas y tiempo",
        clientes_mcp: &["qwen-code-mcp-client", "qwen-code"],
        opcion_prompt: Some("-i"),
    },
    FichaAgente { id: "codex", nombre: "Codex", roles: AMBOS, topes: "solo tiempo", clientes_mcp: &["codex-mcp-client"], opcion_prompt: None },
    FichaAgente {
        id: "opencode",
        nombre: "OpenCode",
        roles: AMBOS,
        topes: "solo tiempo",
        clientes_mcp: &["opencode"],
        opcion_prompt: Some("--prompt"),
    },
    // `agy` 1.4 se presenta como `antigravity-client` (capturado del `initialize`).
    FichaAgente {
        id: "antigravity",
        nombre: "Antigravity",
        roles: PLANIFICADOR,
        topes: "—",
        clientes_mcp: &["antigravity-client"],
        opcion_prompt: Some("-i"),
    },
];

pub fn ficha(id: &str) -> Option<&'static FichaAgente> {
    CATALOGO.iter().find(|f| f.id == id)
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
                let config = &self.agentes[f.id];
                VistaAgente {
                    id: f.id,
                    nombre: f.nombre,
                    admite: f.roles.iter().map(|r| r.nombre()).collect(),
                    rol: self.rol_de(f.id).map(Rol::nombre),
                    bin: config.bin.clone(),
                    ruta: buscar_binario(&config.bin, path).map(|p| p.display().to_string()),
                    perfil: config.modelo.clone(),
                    modelo: self.modelo_de(f.id).cloned(),
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
}
