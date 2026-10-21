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

    /// Nombre en disco: cola, región y `creada_por`.
    pub fn as_str(self) -> &'static str {
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
}

impl FichaAgente {
    pub fn admite(&self, rol: Rol) -> bool {
        self.roles.contains(&rol)
    }
}

const AMBOS: &[Rol] = &[Rol::Planificador, Rol::Ejecutor];
const PLANIFICADOR: &[Rol] = &[Rol::Planificador];

/// Ids del catálogo, en el orden en que se muestran.
pub const IDS_AGENTES: &[&str] = &["claude-code", "qwen-code", "codex", "opencode", "antigravity"];

pub static CATALOGO: &[FichaAgente] = &[
    FichaAgente { id: "claude-code", nombre: "Claude Code", roles: PLANIFICADOR, topes: "—", clientes_mcp: &["claude-code"] },
    FichaAgente {
        id: "qwen-code",
        nombre: "Qwen Code",
        roles: AMBOS,
        topes: "turnos, llamadas y tiempo",
        clientes_mcp: &["qwen-code-mcp-client", "qwen-code"],
    },
    FichaAgente { id: "codex", nombre: "Codex", roles: AMBOS, topes: "solo tiempo", clientes_mcp: &["codex-mcp-client"] },
    FichaAgente { id: "opencode", nombre: "OpenCode", roles: AMBOS, topes: "solo tiempo", clientes_mcp: &["opencode"] },
    // `agy` 1.4 se presenta como `antigravity-client` (capturado del `initialize`).
    FichaAgente { id: "antigravity", nombre: "Antigravity", roles: PLANIFICADOR, topes: "—", clientes_mcp: &["antigravity-client"] },
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

/// La ruta del binario: tal cual si lleva `/`, si no, en el `PATH` dado.
pub fn buscar_binario(bin: &str, path: Option<&std::ffi::OsStr>) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    let ejecutable = |p: &Path| p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
    if bin.is_empty() {
        return None;
    }
    if bin.contains('/') {
        let p = PathBuf::from(bin);
        return ejecutable(&p).then_some(p);
    }
    std::env::split_paths(path?).map(|d| d.join(bin)).find(|p| ejecutable(p))
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
