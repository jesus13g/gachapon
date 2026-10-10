use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("tarea #{0} no encontrada")]
    TaskNotFound(i64),

    #[error("evento #{0} no encontrado")]
    EventNotFound(i64),

    #[error("ejecución de rutina #{0} no encontrada")]
    RoutineRunNotFound(i64),

    #[error("propuesta #{0} no encontrada")]
    PropuestaNotFound(i64),

    #[error("valor de estado inválido: {0}")]
    InvalidStatus(String),

    #[error("valor de origen inválido: {0}")]
    InvalidOrigin(String),

    #[error("valor de resultado inválido: {0}")]
    InvalidOutcome(String),

    #[error("error de base de datos: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("error de migración: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("error de formato de fecha: {0}")]
    TimeFormat(#[from] time::error::Format),

    #[error("error de parseo de fecha: {0}")]
    TimeParse(#[from] time::error::Parse),

    #[error("no se pudo resolver el directorio de datos de la aplicación")]
    NoDataDir,

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;
