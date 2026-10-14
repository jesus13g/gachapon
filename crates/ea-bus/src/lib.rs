//! Bus de ficheros entre el agente local y Claude Code: una cola por agente,
//! el estado de cada trabajo y un semáforo que da a un agente acceso
//! exclusivo de escritura a todo el worktree. Los ficheros son la fuente de
//! verdad; SQLite (`ea-store`) guarda el historial y el diario.

mod bus;
mod candado;
pub mod config;
mod error;
mod fs_atomico;
mod tipos;

pub use bus::{Bus, EntradaCola, ENV_ESTADO, raiz_estado};
pub use candado::{CandadoWorktree, Ocupante, listar_semaforos, listar_semaforos_en};
pub use config::ConfigOrquestador;
pub use error::{ErrorBus, Resultado};
pub use tipos::{
    Agente, Estado, EstadoTrabajo, Evento, Fase, Informe, Modo, Orden, ResultadoInforme, RutinaRef, Topes, transicion,
};
