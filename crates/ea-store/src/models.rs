use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::error::StoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Pendiente,
    EnProgreso,
    Completada,
    Fallida,
    Cancelada,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pendiente => "pendiente",
            TaskStatus::EnProgreso => "en_progreso",
            TaskStatus::Completada => "completada",
            TaskStatus::Fallida => "fallida",
            TaskStatus::Cancelada => "cancelada",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Completada | TaskStatus::Fallida | TaskStatus::Cancelada
        )
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pendiente" => Ok(TaskStatus::Pendiente),
            "en_progreso" => Ok(TaskStatus::EnProgreso),
            "completada" => Ok(TaskStatus::Completada),
            "fallida" => Ok(TaskStatus::Fallida),
            "cancelada" => Ok(TaskStatus::Cancelada),
            other => Err(StoreError::InvalidStatus(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskOrigin {
    Manual,
    Boton,
    Voz,
    Programado,
    Agente,
}

impl TaskOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskOrigin::Manual => "manual",
            TaskOrigin::Boton => "boton",
            TaskOrigin::Voz => "voz",
            TaskOrigin::Programado => "programado",
            TaskOrigin::Agente => "agente",
        }
    }
}

impl std::str::FromStr for TaskOrigin {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manual" => Ok(TaskOrigin::Manual),
            "boton" => Ok(TaskOrigin::Boton),
            "voz" => Ok(TaskOrigin::Voz),
            "programado" => Ok(TaskOrigin::Programado),
            "agente" => Ok(TaskOrigin::Agente),
            other => Err(StoreError::InvalidOrigin(other.to_string())),
        }
    }
}

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
pub struct Task {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub created_by: String,
    pub assigned_to: String,
    pub origin: TaskOrigin,
    pub parent_task_id: Option<i64>,
    pub title: String,
    pub request: String,
    pub status: TaskStatus,
    pub result: Option<String>,
}

/// Datos necesarios para crear una tarea. Deliberadamente el mismo tipo,
/// venga la tarea de un botón, una orden de voz o un disparador programado:
/// es la primitiva única (§9.1 de la especificación).
#[derive(Debug, Clone)]
pub struct NewTask {
    pub created_by: String,
    pub assigned_to: String,
    pub origin: TaskOrigin,
    pub parent_task_id: Option<i64>,
    pub title: String,
    pub request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub actor: String,
    pub action: String,
    pub task_id: Option<i64>,
    pub outcome: Option<EventOutcome>,
    pub duration_ms: Option<i64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub actor: String,
    pub action: String,
    pub task_id: Option<i64>,
    pub outcome: Option<EventOutcome>,
    pub duration_ms: Option<i64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunOutcome {
    Ok,
    Fallida,
}

impl RunOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            RunOutcome::Ok => "ok",
            RunOutcome::Fallida => "fallida",
        }
    }
}

impl std::str::FromStr for RunOutcome {
    type Err = StoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(RunOutcome::Ok),
            "fallida" => Ok(RunOutcome::Fallida),
            other => Err(StoreError::InvalidOutcome(other.to_string())),
        }
    }
}

/// Una ejecución de rutina (§9.2, §10.2). `task_id` enlaza con la tarea que
/// el disparador encoló; `outcome` es `None` mientras la ejecución está en
/// curso.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRun {
    pub id: i64,
    pub routine_slug: String,
    pub task_id: Option<i64>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,
    pub outcome: Option<RunOutcome>,
    pub pasos_ejecutados: i64,
    pub llamadas_modelo: i64,
    pub detalle: Option<String>,
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

/// Acción mutante que una rutina desatendida (o un agente) no ejecutó,
/// encolada para que el usuario decida (§11.2, §12.3). Las propuestas
/// antiguas son solo texto (`accion = None`): aceptarlas deja constancia.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Propuesta {
    pub id: i64,
    pub routine_run_id: Option<i64>,
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
    pub routine_run_id: Option<i64>,
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
