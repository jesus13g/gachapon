use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ErrorBiblioteca {
    #[error("id de elemento inválido {id:?}: {motivo}")]
    IdInvalido { id: String, motivo: String },
    #[error("no existe el elemento {0:?} en el inventario")]
    NoExiste(String),
    /// El elemento no admite el cambio pedido (sin interruptor, solo sesión,
    /// ya está en ese estado…).
    #[error("no se puede cambiar: {0}")]
    NoSePuede(String),
    /// El estado actual ya no es el de la simulación.
    #[error("el estado cambió desde la simulación: {0}")]
    Revalidacion(String),
    /// El sitio al que vuelve (o va) un elemento movido ya está ocupado.
    #[error("destino ocupado: {0}")]
    Ocupado(String),
    #[error("binario no disponible: {0}")]
    BinarioNoDisponible(String),
    #[error("la orden falló: {0}")]
    Orden(String),
    #[error("error de ficheros: {0}")]
    Io(String),
    #[error("ajustes: {0}")]
    Ajustes(String),
    /// Una sesión que no se puede planear o materializar (spec 018, F5).
    #[error("sesión: {0}")]
    Sesion(String),
}

impl From<std::io::Error> for ErrorBiblioteca {
    fn from(e: std::io::Error) -> Self {
        ErrorBiblioteca::Io(e.to_string())
    }
}
