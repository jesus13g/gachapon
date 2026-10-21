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
    FichaAgente { id: "claude-code", nombre: "Claude Code", roles: PLANIFICADOR, topes: "—" },
    FichaAgente { id: "qwen-code", nombre: "Qwen Code", roles: AMBOS, topes: "turnos, llamadas y tiempo" },
    FichaAgente { id: "codex", nombre: "Codex", roles: AMBOS, topes: "solo tiempo" },
    FichaAgente { id: "opencode", nombre: "OpenCode", roles: AMBOS, topes: "solo tiempo" },
    FichaAgente { id: "antigravity", nombre: "Antigravity", roles: PLANIFICADOR, topes: "—" },
];

pub fn ficha(id: &str) -> Option<&'static FichaAgente> {
    CATALOGO.iter().find(|f| f.id == id)
}

/// Id válido de un perfil de modelo: minúsculas, dígitos, `-` y `_`, hasta
/// 32 caracteres (es un trozo de clave de `ajustes.toml`).
pub fn id_perfil_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
}
