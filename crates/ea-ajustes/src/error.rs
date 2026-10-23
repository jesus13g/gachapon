use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorAjustes {
    #[error("no existe el ajuste {0:?}")]
    ClaveDesconocida(String),
    #[error("valor inválido para {clave}: {motivo}")]
    ValorInvalido { clave: String, motivo: String },
    #[error("{clave} no se puede cambiar desde {canal}: se cambia en una terminal con `ea ajustes establecer`")]
    NoEditable { clave: String, canal: String },
    #[error("{clave} solo se puede endurecer desde aquí (valor actual {actual}); para subirlo usa `ea ajustes establecer` en una terminal")]
    SoloEndurecer { clave: String, actual: String },
    #[error("{clave} lo fija la variable de entorno {variable}; no se cambia desde ningún canal mientras esté definida")]
    FijadaPorEntorno { clave: String, variable: String },
    #[error("ajustes.toml ha cambiado desde que se leyó; vuelve a cargarlo")]
    Version,
    #[error("no hay cambios que aplicar")]
    SinCambios,
    #[error("ajustes.toml no es válido: {0}")]
    FicheroInvalido(String),
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
}
