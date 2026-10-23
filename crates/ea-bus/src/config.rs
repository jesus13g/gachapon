//! Configuración del ejecutor: la parte de los ajustes (`ea-ajustes`) que
//! necesita para lanzar al agente que ocupa el rol de ejecutor. Los modelos
//! se nombran en `ajustes.toml` (perfiles `modelos.*`, o los heredados
//! `modelo.*` y `orquestador.toml`); en el código solo aparecen en los
//! valores por defecto de `ea-ajustes`.

use ea_ajustes::{Ajustes, Fuentes, PerfilModelo, Rol};
use serde::Serialize;

use crate::skills::ConfigSkills;
use crate::tipos::{EjecutadoPor, Topes};

/// El adaptador de cada agente que puede ejecutar (spec 017): cada CLI tiene
/// sus argumentos y su formato de salida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TipoAgente {
    QwenCode,
    Codex,
    OpenCode,
}

impl TipoAgente {
    /// El tipo de un agente del catálogo que admite el rol de ejecutor.
    pub fn desde(id: &str) -> Option<Self> {
        match id {
            "qwen-code" => Some(Self::QwenCode),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::QwenCode => "qwen-code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
    }
}

/// El agente que ocupa el rol de ejecutor, con su binario y su modelo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfilEjecutor {
    /// Id del catálogo (`qwen-code`, `codex`, `opencode`).
    pub id: String,
    pub tipo: TipoAgente,
    pub bin: String,
    /// Siempre local: `ea-ajustes` rechaza un ejecutor con modelo remoto.
    pub modelo: PerfilModelo,
}

impl PerfilEjecutor {
    /// Lo que se anota en el trabajo al tomarlo.
    pub fn ejecutado_por(&self) -> EjecutadoPor {
        EjecutadoPor { agente: self.id.clone(), modelo: self.modelo.nombre.clone() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigOrquestador {
    pub ejecutor: PerfilEjecutor,
    /// Techo de los topes de cualquier orden.
    pub techo_topes: Topes,
    pub avisos_escritorio: bool,
    /// Ejecuta al agente dentro de bubblewrap (solo el worktree es escribible).
    pub sandbox: bool,
    /// Catálogo de skills del ejecutor, cuáles se pueden usar y quién las elige.
    pub skills: ConfigSkills,
}

fn techo(a: &Ajustes) -> Topes {
    let t = a.agente.techo_topes;
    Topes { turnos: t.turnos, tiempo_s: t.tiempo_s, llamadas_herramienta: t.llamadas_herramienta, iteraciones: t.iteraciones }
}

fn ejecutor(a: &Ajustes) -> PerfilEjecutor {
    let (id, config) = a.agente_de(Rol::Ejecutor);
    // `validar_roles` garantiza que el ejecutor es de uno de estos tipos y
    // que tiene un perfil local; si no, el de fábrica.
    match (TipoAgente::desde(id), a.modelo_de(id)) {
        (Some(tipo), Some(modelo)) => {
            PerfilEjecutor { id: id.to_string(), tipo, bin: config.bin.clone(), modelo: modelo.clone() }
        }
        _ => ejecutor(&Ajustes::por_defecto(&Fuentes::sistema())),
    }
}

impl ConfigOrquestador {
    pub fn desde_ajustes(a: &Ajustes) -> Self {
        let s = &a.agente.skills;
        Self {
            ejecutor: ejecutor(a),
            techo_topes: techo(a),
            avisos_escritorio: a.agente.avisos_escritorio,
            sandbox: a.agente.sandbox,
            skills: ConfigSkills {
                dir: s.dir.clone(),
                habilitadas: s.habilitadas.clone(),
                max_por_trabajo: s.max_por_trabajo,
                max_caracteres: s.max_caracteres,
                pueden_elegir: s.pueden_elegir.clone(),
                planificador: a.roles.planificador.clone(),
            },
        }
    }

    /// La configuración de un ejecutor concreto (spec 019, RF-008): la misma
    /// que [`ConfigOrquestador::desde_ajustes`] pero con la ficha y el perfil
    /// de modelo local del agente `id` (`agentes.<id>.bin`), no los de
    /// `roles.ejecutor`. `None` si `id` no es un agente que pueda ejecutar o
    /// su perfil de modelo no existe o es remoto.
    pub fn para_agente(a: &Ajustes, id: &str) -> Option<Self> {
        let tipo = TipoAgente::desde(id)?;
        let modelo = a.modelo_de(id).filter(|m| !m.remoto)?;
        let bin = a.agentes.get(id)?.bin.clone();
        Some(Self { ejecutor: PerfilEjecutor { id: id.to_string(), tipo, bin, modelo: modelo.clone() }, ..Self::desde_ajustes(a) })
    }

    /// Los ajustes efectivos de ahora (se relee en cada ciclo del ejecutor).
    pub fn cargar() -> Self {
        Self::desde_ajustes(&ea_ajustes::cargar())
    }
}

impl Default for ConfigOrquestador {
    fn default() -> Self {
        Self::desde_ajustes(&Ajustes::por_defecto(&Fuentes::sistema()))
    }
}
