use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ErrorBiblioteca {
    #[error("id de elemento inválido {id:?}: {motivo}")]
    IdInvalido { id: String, motivo: String },
    #[error("no existe el elemento {0:?} en el inventario")]
    NoExiste(String),
    /// Una sesión que no se puede planear o materializar (spec 018, F5).
    #[error("sesión: {0}")]
    Sesion(String),
}
