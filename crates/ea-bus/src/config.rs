//! Configuración del ejecutor: la parte de los ajustes (`ea-ajustes`) que
//! necesita para lanzar al agente local. El modelo se nombra en
//! `ajustes.toml` (o en el heredado `orquestador.toml`); en el código solo
//! aparece en los valores por defecto de `ea-ajustes`.

use std::path::{Path, PathBuf};

use ea_ajustes::{Ajustes, Fuentes};
use serde::{Deserialize, Deserializer};

use crate::skills::ConfigSkills;
use crate::tipos::Topes;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ConfigOrquestador {
    pub base_url: String,
    pub modelo: String,
    pub qwen_bin: String,
    /// Techo de los topes de cualquier orden (en `orquestador.toml` se llamaba
    /// `[topes]` y no se usaba).
    #[serde(alias = "topes", deserialize_with = "techo_parcial")]
    pub techo_topes: Topes,
    pub avisos_escritorio: bool,
    /// Ejecuta qwen dentro de bubblewrap (solo el worktree es escribible).
    pub sandbox: bool,
    /// Catálogo de skills de Qwen, cuáles se pueden usar y quién las elige.
    pub skills: ConfigSkills,
}

fn techo(a: &Ajustes) -> Topes {
    let t = a.agente.techo_topes;
    Topes { turnos: t.turnos, tiempo_s: t.tiempo_s, llamadas_herramienta: t.llamadas_herramienta, iteraciones: t.iteraciones }
}

/// Un `[topes]` a medias completa lo que falta con el techo duro, no con los
/// topes por defecto de una orden.
fn techo_parcial<'de, D: Deserializer<'de>>(d: D) -> Result<Topes, D::Error> {
    #[derive(Deserialize)]
    struct Parcial {
        turnos: Option<u32>,
        tiempo_s: Option<u64>,
        llamadas_herramienta: Option<u32>,
        iteraciones: Option<u32>,
    }
    let p = Parcial::deserialize(d)?;
    let t = ea_ajustes::TECHO_DURO;
    Ok(Topes {
        turnos: p.turnos.unwrap_or(t.turnos),
        tiempo_s: p.tiempo_s.unwrap_or(t.tiempo_s),
        llamadas_herramienta: p.llamadas_herramienta.unwrap_or(t.llamadas_herramienta),
        iteraciones: p.iteraciones.unwrap_or(t.iteraciones),
    })
}

impl ConfigOrquestador {
    pub fn desde_ajustes(a: &Ajustes) -> Self {
        let s = &a.agente.skills;
        let perfil = a.modelo_ejecutor();
        Self {
            base_url: perfil.base_url.clone(),
            modelo: perfil.nombre.clone(),
            qwen_bin: a.agentes["qwen-code"].bin.clone(),
            techo_topes: techo(a),
            avisos_escritorio: a.agente.avisos_escritorio,
            sandbox: a.agente.sandbox,
            skills: ConfigSkills {
                dir: s.dir.clone(),
                habilitadas: s.habilitadas.clone(),
                max_por_trabajo: s.max_por_trabajo,
                max_caracteres: s.max_caracteres,
                pueden_elegir: s.pueden_elegir.clone(),
            },
        }
    }

    /// Los ajustes efectivos de ahora (se relee en cada ciclo del ejecutor).
    pub fn cargar() -> Self {
        Self::desde_ajustes(&ea_ajustes::cargar())
    }

    /// Lee un `orquestador.toml` con su formato antiguo (fuente heredada);
    /// si falta o es inválido, los valores por defecto.
    pub fn cargar_de(ruta: &Path) -> Self {
        match std::fs::read_to_string(ruta) {
            Ok(texto) => toml::from_str(&texto).unwrap_or_else(|e| {
                eprintln!("orquestador.toml inválido ({}): {e}; usando los valores por defecto", ruta.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
}

impl Default for ConfigOrquestador {
    fn default() -> Self {
        Self::desde_ajustes(&Ajustes::por_defecto(&Fuentes::sistema()))
    }
}

/// El `orquestador.toml` heredado.
pub fn ruta_config() -> Option<PathBuf> {
    Some(Fuentes::sistema().dir_config.join("orquestador.toml"))
}
