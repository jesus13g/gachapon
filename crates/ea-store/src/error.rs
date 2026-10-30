use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("evento #{0} no encontrado")]
    EventNotFound(i64),

    #[error("propuesta #{0} no encontrada")]
    PropuestaNotFound(i64),

    #[error("propuesta #{0} ya decidida")]
    PropuestaYaDecidida(i64),

    #[error("la propuesta #{0} lleva una acción: aceptarla es ejecutarla, no solo marcarla")]
    PropuestaConAccion(i64),

    #[error("la propuesta #{0} no lleva ninguna acción que ejecutar")]
    PropuestaSinAccion(i64),

    #[error("nota del diario no válida: {0}")]
    NotaInvalida(String),

    #[error("valor de estado inválido: {0}")]
    InvalidStatus(String),

    #[error("valor de resultado inválido: {0}")]
    InvalidOutcome(String),

    #[error("base de datos del esquema antiguo: haz copia y bórrala (ecosistema.db*) para empezar de cero")]
    EsquemaAntiguo,

    #[error("error de base de datos: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("error de migración: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("error de formato de fecha: {0}")]
    TimeFormat(#[from] time::error::Format),

    #[error("error de parseo de fecha: {0}")]
    TimeParse(#[from] time::error::Parse),

    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;
