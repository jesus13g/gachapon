use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorBoveda {
    /// La ruta no es una ruta relativa limpia de nota (vacía, absoluta, con
    /// `..`, sin extensión `.md`, con componentes ocultos…).
    #[error("ruta no válida: {0}")]
    Ruta(String),

    /// La ruta, una vez resueltos los enlaces simbólicos, saldría de su región.
    #[error("la ruta sale de la región de la bóveda: {0}")]
    FueraDeLaBoveda(String),

    #[error("argumentos no válidos: {0}")]
    Argumentos(String),

    #[error("cabecera no válida: {0}")]
    Cabecera(String),

    #[error("la nota no existe: {0}")]
    NoExiste(String),

    #[error("ya existe: {0}")]
    YaExiste(String),

    /// Acción que ese actor o esa región no admiten (p. ej. un agente
    /// escribiendo en la región compartida).
    #[error("no permitido: {0}")]
    Prohibida(String),

    /// El contenido cambió entre la simulación y la ejecución.
    #[error("el contenido cambió desde la simulación: {0}")]
    Cambio(String),

    /// Otro proceso tiene el escritor; no se obtuvo en el tiempo de espera.
    #[error("el escritor de la bóveda está ocupado (otro proceso escribe); inténtalo de nuevo")]
    Ocupada,

    #[error("error de E/S en la bóveda: {0}")]
    Io(String),

    /// La nota se escribió pero no se pudo anotar en el diario.
    #[error("la nota se escribió ({0}) pero no se pudo anotar en el diario: {1}")]
    EventoPerdido(String, String),

    #[error(transparent)]
    Store(#[from] ea_store::StoreError),
}

impl From<std::io::Error> for ErrorBoveda {
    fn from(e: std::io::Error) -> Self {
        ErrorBoveda::Io(e.to_string())
    }
}

pub type Resultado<T> = Result<T, ErrorBoveda>;
