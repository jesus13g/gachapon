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

impl From<ea_boveda::ErrorBoveda> for ErrorAccion {
    fn from(e: ea_boveda::ErrorBoveda) -> Self {
        use ea_boveda::ErrorBoveda as B;
        let m = e.to_string();
        match e {
            B::Ruta(_) | B::Argumentos(_) | B::Cabecera(_) => ErrorAccion::Argumentos(m),
            B::NoExiste(_) | B::YaExiste(_) | B::Cambio(_) | B::Ocupada => ErrorAccion::NoAplica(m),
            B::Prohibida(_) | B::FueraDeLaBoveda(_) => ErrorAccion::Prohibida(m),
            B::Store(s) => ErrorAccion::Store(s),
            B::Io(_) | B::EventoPerdido(..) => ErrorAccion::Sistema(m),
        }
    }
}
