use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ErrorBiblioteca {
    #[error("id de elemento inválido {id:?}: {motivo}")]
    IdInvalido { id: String, motivo: String },
    #[error("no existe el elemento {0:?} en el inventario")]
    NoExiste(String),
}
