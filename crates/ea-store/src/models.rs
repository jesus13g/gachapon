use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventOutcome {
    Ok,
    Error,
}

impl EventOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            EventOutcome::Ok => "ok",
            EventOutcome::Error => "error",
        }
    }
}

impl std::str::FromStr for EventOutcome {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(EventOutcome::Ok),
            "error" => Ok(EventOutcome::Error),
            other => Err(StoreError::InvalidOutcome(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub actor: String,
    pub action: String,
    /// Id del trabajo del bus al que se asocia el evento.
    pub trabajo: Option<String>,
    pub outcome: Option<EventOutcome>,
    pub duration_ms: Option<i64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub actor: String,
    pub action: String,
    /// Id del trabajo del bus al que se asocia el evento.
    pub trabajo: Option<String>,
    pub outcome: Option<EventOutcome>,
    pub duration_ms: Option<i64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoPropuesta {
    Pendiente,
    Aceptada,
    Rechazada,
}

impl EstadoPropuesta {
    pub fn as_str(self) -> &'static str {
        match self {
            EstadoPropuesta::Pendiente => "pendiente",
            EstadoPropuesta::Aceptada => "aceptada",
            EstadoPropuesta::Rechazada => "rechazada",
        }
    }
}

impl std::str::FromStr for EstadoPropuesta {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pendiente" => Ok(EstadoPropuesta::Pendiente),
            "aceptada" => Ok(EstadoPropuesta::Aceptada),
            "rechazada" => Ok(EstadoPropuesta::Rechazada),
            other => Err(StoreError::InvalidStatus(other.to_string())),
        }
    }
}

/// Resultado de ejecutar la acción de una propuesta aceptada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoEjecucion {
    /// Reclamada por quien la ejecuta; si el proceso muere aquí se queda así
    /// y nunca se reintenta sola (una acción irreversible no se repite).
    Ejecutando,
    Ejecutada,
    Fallida,
}

impl EstadoEjecucion {
    pub fn as_str(self) -> &'static str {
        match self {
            EstadoEjecucion::Ejecutando => "ejecutando",
            EstadoEjecucion::Ejecutada => "ejecutada",
            EstadoEjecucion::Fallida => "fallida",
        }
    }
}

impl std::str::FromStr for EstadoEjecucion {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ejecutando" => Ok(EstadoEjecucion::Ejecutando),
            "ejecutada" => Ok(EstadoEjecucion::Ejecutada),
            "fallida" => Ok(EstadoEjecucion::Fallida),
            other => Err(StoreError::InvalidStatus(other.to_string())),
        }
    }
}

/// Acción estructurada de una propuesta: qué herramienta, con qué
/// argumentos, su nivel de riesgo (`reversible` / `irreversible`), el texto
/// de la simulación que vio quien la propuso y la precondición (estado del
/// mundo en la simulación) contra la que se revalida antes de ejecutar.
/// `ea-store` solo la guarda; su significado vive en `ea-acciones`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccionPropuesta {
    pub herramienta: String,
    pub argumentos: serde_json::Value,
    pub riesgo: String,
    pub simulacion: String,
    pub precondicion: serde_json::Value,
}

/// Ejecución de la acción de una propuesta aceptada.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EjecucionPropuesta {
    pub estado: EstadoEjecucion,
    pub detalle: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub ejecutada_en: Option<OffsetDateTime>,
}

/// Acción mutante que un agente no ejecutó,
/// encolada para que el usuario decida. Las propuestas
/// antiguas son solo texto (`accion = None`): aceptarlas deja constancia.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Propuesta {
    pub id: i64,
    /// Actor que la encoló (`orquestador`, `claude-code`, `modelo-local`).
    pub proponente: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub creado_en: OffsetDateTime,
    pub descripcion: String,
    pub estado: EstadoPropuesta,
    #[serde(with = "time::serde::rfc3339::option")]
    pub decidido_en: Option<OffsetDateTime>,
    pub accion: Option<AccionPropuesta>,
    pub ejecucion: Option<EjecucionPropuesta>,
}

/// Datos para encolar una propuesta.
#[derive(Debug, Clone)]
pub struct NuevaPropuesta {
    pub proponente: String,
    pub descripcion: String,
    pub accion: Option<AccionPropuesta>,
}

/// Fila del índice de la bóveda: los metadatos de cabecera de una nota que
/// interesa filtrar y mostrar sin abrir el fichero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntradaIndice {
    /// Relativa a la raíz de la bóveda, con `/`.
    pub ruta: String,
    pub region: String,
    pub autor: Option<String>,
    pub titulo: String,
    pub origen: Option<String>,
    pub promocion: Option<String>,
    pub creada: Option<String>,
    pub actualizada: Option<String>,
    /// Etiquetas separadas por espacios, solo para la búsqueda.
    pub tags: String,
    pub mtime_ns: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RutaIndexada {
    pub ruta: String,
    pub mtime_ns: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultadoBusqueda {
    pub ruta: String,
    pub region: String,
    pub autor: Option<String>,
    pub titulo: String,
    pub actualizada: Option<String>,
    pub promocion: Option<String>,
    /// Trozo del cuerpo con los términos marcados entre «»; vacío si no hubo
    /// consulta.
    pub fragmento: String,
}
