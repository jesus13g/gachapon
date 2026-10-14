//! Tipos del bus y máquina de estados pura.

use serde::{Deserialize, Serialize};

use crate::error::{ErrorBus, Resultado};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agente {
    #[serde(rename = "modelo-local")]
    ModeloLocal,
    #[serde(rename = "claude-code")]
    ClaudeCode,
}

impl Agente {
    pub fn as_str(self) -> &'static str {
        match self {
            Agente::ModeloLocal => "modelo-local",
            Agente::ClaudeCode => "claude-code",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modo {
    Mision,
    Obra,
    Rutina,
}

impl Modo {
    pub fn as_str(self) -> &'static str {
        match self {
            Modo::Mision => "mision",
            Modo::Obra => "obra",
            Modo::Rutina => "rutina",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Topes {
    pub turnos: u32,
    pub tiempo_s: u64,
    pub llamadas_herramienta: u32,
    pub iteraciones: u32,
}

impl Default for Topes {
    fn default() -> Self {
        Self { turnos: 30, tiempo_s: 900, llamadas_herramienta: 60, iteraciones: 5 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fase {
    pub numero: u32,
    pub titulo: String,
    pub instrucciones: String,
    pub entregable: String,
    /// Comando de shell que decide si la fase está hecha.
    pub criterio: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RutinaRef {
    pub slug: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Orden {
    pub id: String,
    pub modo: Modo,
    pub titulo: String,
    /// Ruta absoluta.
    pub worktree: String,
    /// Texto para el agente.
    pub objetivo: String,
    /// Comando de shell opcional; en la obra cada fase tiene el suyo.
    #[serde(default)]
    pub criterio_exito: Option<String>,
    #[serde(default)]
    pub fases: Vec<Fase>,
    #[serde(default)]
    pub revisar_cada: Option<u32>,
    #[serde(default)]
    pub topes: Topes,
    #[serde(default)]
    pub rutina: Option<RutinaRef>,
    /// Notas que añade la revisión al reformular.
    #[serde(default)]
    pub notas_revision: Vec<String>,
    pub creada_en: String,
    pub creada_por: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoTrabajo {
    EnCola,
    Ejecutando,
    EnRevision,
    EnEspera,
    Cerrado,
    Cancelado,
}

impl EstadoTrabajo {
    pub fn as_str(self) -> &'static str {
        match self {
            EstadoTrabajo::EnCola => "en_cola",
            EstadoTrabajo::Ejecutando => "ejecutando",
            EstadoTrabajo::EnRevision => "en_revision",
            EstadoTrabajo::EnEspera => "en_espera",
            EstadoTrabajo::Cerrado => "cerrado",
            EstadoTrabajo::Cancelado => "cancelado",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Estado {
    pub estado: EstadoTrabajo,
    #[serde(default)]
    pub fase_actual: Option<u32>,
    #[serde(default)]
    pub iteracion: u32,
    pub actualizado_en: String,
    #[serde(default)]
    pub detalle: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultadoInforme {
    Terminado,
    Bloqueado,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Informe {
    pub resultado: ResultadoInforme,
    /// Salida del criterio, recortada.
    #[serde(default)]
    pub evidencia: String,
    #[serde(default)]
    pub commit_inicio: Option<String>,
    #[serde(default)]
    pub diff_stat: Option<String>,
    #[serde(default)]
    pub motivo: Option<String>,
    #[serde(default)]
    pub fases_hechas: u32,
    #[serde(default)]
    pub duracion_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evento {
    Tomar,
    Entregar(Informe),
    Reformular,
    Cerrar,
    Cancelar,
    PonerEnEspera,
}

impl Evento {
    pub fn nombre(&self) -> &'static str {
        match self {
            Evento::Tomar => "tomar",
            Evento::Entregar(_) => "entregar",
            Evento::Reformular => "reformular",
            Evento::Cerrar => "cerrar",
            Evento::Cancelar => "cancelar",
            Evento::PonerEnEspera => "poner_en_espera",
        }
    }
}

/// Máquina de estados pura.
pub fn transicion(estado: EstadoTrabajo, evento: &Evento) -> Resultado<EstadoTrabajo> {
    use EstadoTrabajo::*;
    let nuevo = match (estado, evento) {
        (EnCola, Evento::Tomar) => Ejecutando,
        (Ejecutando, Evento::Entregar(_)) => EnRevision,
        (EnRevision | EnEspera, Evento::Reformular) => EnCola,
        (EnRevision | EnEspera, Evento::Cerrar) => Cerrado,
        (Ejecutando | EnRevision, Evento::PonerEnEspera) => EnEspera,
        (EnCola | Ejecutando | EnRevision | EnEspera, Evento::Cancelar) => Cancelado,
        _ => {
            return Err(ErrorBus::TransicionInvalida {
                desde: estado.as_str().into(),
                evento: evento.nombre().into(),
            });
        }
    };
    Ok(nuevo)
}
