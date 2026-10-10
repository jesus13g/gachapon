use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorAccion {
    #[error("herramienta desconocida: '{0}'")]
    Desconocida(String),

    #[error("'{0}' no es una herramienta mutante supervisada")]
    NoSupervisada(String),

    #[error("argumentos inválidos: {0}")]
    Argumentos(String),

    /// El estado del mundo no admite la acción (en la simulación, o al
    /// revalidar antes de ejecutar porque cambió desde entonces).
    #[error("la acción no aplica: {0}")]
    NoAplica(String),

    /// El canal o el actor no pueden hacer esto (política de riesgo §11.1).
    #[error("no permitido: {0}")]
    Prohibida(String),

    #[error("fallo del sistema: {0}")]
    Sistema(String),

    #[error(transparent)]
    Store(#[from] ea_store::StoreError),
}
