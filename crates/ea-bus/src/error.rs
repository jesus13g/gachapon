use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorBus {
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("almacén: {0}")]
    Store(#[from] ea_store::StoreError),
    #[error("transición inválida: {evento} desde {desde}")]
    TransicionInvalida { desde: String, evento: String },
    #[error("no existe el trabajo {0}")]
    NoExiste(String),
    #[error("el worktree está ocupado")]
    Ocupado,
    #[error("{0}")]
    Argumentos(String),
}

pub type Resultado<T> = Result<T, ErrorBus>;
