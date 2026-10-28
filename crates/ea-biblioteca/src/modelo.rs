//! El modelo de la biblioteca: elementos, ids estables y su serialización.
//!
//! # Id estable
//!
//! `<agente>/<tipo>/<ámbito>/<nombre>`, por ejemplo
//! `claude-code/plugin/usuario/statuspane@claude-statuspane` o
//! `claude-code/skill/plugin:foo@mkt/revisar`.
//!
//! Reglas (las valida [`Id::nuevo`] y las reutiliza [`Id::analizar`]):
//! - `agente`: un id del catálogo de agentes o `ecosistema`.
//! - `tipo`: `skill`, `plugin`, `subagente`, `mcp`, `comando` o `extension`.
//! - `ámbito`: `usuario`, `proyecto`, `integrada`, `ecosistema` o
//!   `plugin:<id>` (el `<id>` no lleva `/`; el `id@mkt` de Claude Code vale).
//! - `nombre`: no vacío, sin caracteres de control ni `\`, sin `..` en ningún
//!   sitio, sin segmentos vacíos ni que empiecen o acaben en `/`. La `/` se
//!   admite solo porque el nombre es el ÚLTIMO componente (analizar corta en
//!   las tres primeras `/`) y hay paquetes con ámbito (`@ámbito/paquete` en
//!   los plugins de OpenCode). **Un nombre nunca se usa como ruta**: la ruta
//!   de origen va aparte, en `Elemento::ruta`.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use ea_ajustes::IDS_AGENTES;
use serde::{Deserialize, Serialize};

use crate::error::ErrorBiblioteca;

/// El «agente» de lo que pone el propio ecosistema (el servidor `ea-mcp` y
/// las skills del repo).
pub const AGENTE_ECOSISTEMA: &str = "ecosistema";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tipo {
    Skill,
    Plugin,
    Subagente,
    Mcp,
    Comando,
    Extension,
}

impl Tipo {
    pub const TODOS: [Tipo; 6] = [Tipo::Skill, Tipo::Plugin, Tipo::Subagente, Tipo::Mcp, Tipo::Comando, Tipo::Extension];

    pub fn as_str(self) -> &'static str {
        match self {
            Tipo::Skill => "skill",
            Tipo::Plugin => "plugin",
            Tipo::Subagente => "subagente",
            Tipo::Mcp => "mcp",
            Tipo::Comando => "comando",
            Tipo::Extension => "extension",
        }
    }

    pub fn desde(texto: &str) -> Option<Tipo> {
        Tipo::TODOS.into_iter().find(|t| t.as_str() == texto)
    }
}

impl fmt::Display for Tipo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ambito {
    Usuario,
    /// El del repo (o carpeta) en que se ejecuta `ea`.
    Proyecto,
    /// Lo que trae un plugin; lleva su id (`id@mkt`).
    Plugin(String),
    /// Viene con el agente (no se toca).
    Integrada,
    /// Lo pone el ecosistema.
    Ecosistema,
}

impl Ambito {
    pub fn texto(&self) -> String {
        match self {
            Ambito::Usuario => "usuario".into(),
            Ambito::Proyecto => "proyecto".into(),
            Ambito::Plugin(id) => format!("plugin:{id}"),
            Ambito::Integrada => "integrada".into(),
            Ambito::Ecosistema => "ecosistema".into(),
        }
    }

    pub fn desde(texto: &str) -> Option<Ambito> {
        match texto {
            "usuario" => Some(Ambito::Usuario),
            "proyecto" => Some(Ambito::Proyecto),
            "integrada" => Some(Ambito::Integrada),
            "ecosistema" => Some(Ambito::Ecosistema),
            otro => {
                let id = otro.strip_prefix("plugin:")?;
                (!id.is_empty() && !id.contains('/') && !id.contains("..") && !id.chars().any(char::is_control)).then(|| Ambito::Plugin(id.to_string()))
            }
        }
    }

    /// La categoría por defecto de un elemento: su origen.
    pub fn categoria(&self) -> String {
        self.texto()
    }
}

impl fmt::Display for Ambito {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.texto())
    }
}

impl Serialize for Ambito {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.texto())
    }
}

impl<'de> Deserialize<'de> for Ambito {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let t = String::deserialize(d)?;
        Ambito::desde(&t).ok_or_else(|| serde::de::Error::custom(format!("ámbito inválido: {t:?}")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Activado {
    Si,
    No,
    Desconocido,
}

impl Activado {
    pub fn desde_bool(b: bool) -> Self {
        if b { Activado::Si } else { Activado::No }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Activado::Si => "si",
            Activado::No => "no",
            Activado::Desconocido => "desconocido",
        }
    }

    pub fn desde(texto: &str) -> Option<Activado> {
        [Activado::Si, Activado::No, Activado::Desconocido].into_iter().find(|a| a.as_str() == texto)
    }
}

/// Cómo se cambia el estado de un elemento (la F4 lo aplica).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mecanismo {
    /// `claude plugin enable|disable`, `qwen extensions …`, `agy mcp …`.
    ComandoOficial,
    /// Un campo `enabled` en la configuración del agente.
    Campo,
    /// Mover el fichero o el enlace al almacén del ecosistema.
    Mover,
    /// `agente.skills.habilitadas` en `ajustes.toml`.
    Ajustes,
    /// Solo se puede acotar en una sesión (`~/.claude.json` no se edita).
    SoloSesion,
    /// No se puede cambiar.
    No,
}

/// Definición redactada de un servidor MCP: lo que F5 necesita para saber
/// qué es, nunca un secreto. De `env` y `headers` solo se guardan los
/// NOMBRES; los argumentos y la URL pasan por [`crate::redactar`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DefMcp {
    /// `stdio`, `http`, `sse` o `desconocido`.
    pub transporte: String,
    pub comando: Option<String>,
    pub args: Vec<String>,
    /// Solo esquema, host y puerto.
    pub url: Option<String>,
    /// Nombres de las variables de entorno (sin valores).
    pub env: Vec<String>,
    /// Nombres de las cabeceras (sin valores).
    pub headers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Elemento {
    pub id: String,
    pub agente: String,
    pub tipo: Tipo,
    pub ambito: Ambito,
    pub nombre: String,
    pub descripcion: String,
    /// Fichero, directorio o enlace de origen (en un MCP, la configuración).
    pub ruta: PathBuf,
    pub activado: Activado,
    pub mecanismo: Mecanismo,
    pub categoria: String,
    /// Versión del agente con la que se comprobó el formato.
    pub version_formato: String,
    /// Solo en los MCP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def_mcp: Option<DefMcp>,
    /// Aviso para la ficha (p. ej. «JSONC con comentarios: no se edita»).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nota: Option<String>,
}

impl Elemento {
    /// Un elemento con valores neutros (`desconocido`, sin mecanismo,
    /// categoría = origen); el lector rellena lo que sabe.
    pub fn nuevo(agente: &str, tipo: Tipo, ambito: Ambito, nombre: &str, ruta: PathBuf) -> Result<Self, ErrorBiblioteca> {
        let id = Id::nuevo(agente, tipo, ambito.clone(), nombre)?;
        Ok(Self {
            id: id.to_string(),
            agente: agente.to_string(),
            tipo,
            categoria: ambito.categoria(),
            ambito,
            nombre: nombre.to_string(),
            descripcion: String::new(),
            ruta,
            activado: Activado::Desconocido,
            mecanismo: Mecanismo::No,
            version_formato: String::new(),
            def_mcp: None,
            nota: None,
        })
    }

    pub fn id_analizado(&self) -> Result<Id, ErrorBiblioteca> {
        Id::analizar(&self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Id {
    pub agente: String,
    pub tipo: Tipo,
    pub ambito: Ambito,
    pub nombre: String,
}

impl Id {
    pub fn nuevo(agente: &str, tipo: Tipo, ambito: Ambito, nombre: &str) -> Result<Id, ErrorBiblioteca> {
        let mal = |motivo: &str| ErrorBiblioteca::IdInvalido { id: format!("{agente}/{tipo}/{ambito}/{nombre}"), motivo: motivo.into() };
        if agente != AGENTE_ECOSISTEMA && !IDS_AGENTES.contains(&agente) {
            return Err(mal("el agente no está en el catálogo"));
        }
        if let Ambito::Plugin(id) = &ambito
            && Ambito::desde(&format!("plugin:{id}")).is_none()
        {
            return Err(mal("id de plugin inválido"));
        }
        if nombre.is_empty() {
            return Err(mal("el nombre está vacío"));
        }
        if nombre.contains("..") {
            return Err(mal("el nombre no puede llevar «..»"));
        }
        if nombre.contains('\\') || nombre.chars().any(char::is_control) {
            return Err(mal("el nombre no puede llevar «\\» ni caracteres de control"));
        }
        if nombre.split('/').any(str::is_empty) {
            return Err(mal("el nombre no puede tener segmentos vacíos"));
        }
        Ok(Id { agente: agente.to_string(), tipo, ambito, nombre: nombre.to_string() })
    }

    pub fn analizar(texto: &str) -> Result<Id, ErrorBiblioteca> {
        let mal = |motivo: &str| ErrorBiblioteca::IdInvalido { id: texto.into(), motivo: motivo.into() };
        let mut p = texto.splitn(4, '/');
        let (Some(agente), Some(tipo), Some(ambito), Some(nombre)) = (p.next(), p.next(), p.next(), p.next()) else {
            return Err(mal("se esperan cuatro partes: agente/tipo/ámbito/nombre"));
        };
        let tipo = Tipo::desde(tipo).ok_or_else(|| mal("tipo desconocido"))?;
        let ambito = Ambito::desde(ambito).ok_or_else(|| mal("ámbito desconocido"))?;
        Id::nuevo(agente, tipo, ambito, nombre)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}/{}", self.agente, self.tipo, self.ambito, self.nombre)
    }
}

impl FromStr for Id {
    type Err = ErrorBiblioteca;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Id::analizar(s)
    }
}
