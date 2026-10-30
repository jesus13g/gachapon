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
    /// La rutina está retirada o no tiene fichero en `rutinas_dir`
    /// ([`crate::Ubicacion::motivo_no_lanzable`]).
    #[error("{0}")]
    NoLanzable(String),
    /// La rutina (su slug) ya tiene una ejecución activa, en cola o
    /// ejecutándose: no se lanza otra ([`crate::Bus::lanzar_rutina`]).
    #[error("la rutina '{0}' ya tiene una ejecución en curso")]
    EnCurso(String),
    /// Las skills pedidas no pasan la llave, no existen, no están
    /// habilitadas o exceden los topes de `[agente.skills]`.
    #[error("skills: {0}")]
    Skills(String),
    /// Falta la instantánea de árbol (o el `commit_inicio`) que pide el
    /// diff de una fase.
    #[error("{0}")]
    SinInstantanea(String),
    /// Una orden `git` del sistema ([`crate::git`]) terminó con error:
    /// sus argumentos y lo que dijo git.
    #[error("git {orden}: {detalle}")]
    Git { orden: String, detalle: String },
    /// Una orden `git` del sistema no terminó dentro de su tope y se mató.
    #[error("git no respondió en {0} s")]
    GitSinRespuesta(u64),
}

pub type Resultado<T> = Result<T, ErrorBus>;
