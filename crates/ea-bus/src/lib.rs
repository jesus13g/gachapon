//! Bus de ficheros entre el agente local y Claude Code: una cola por agente,
//! el estado de cada trabajo y un semáforo que da a un agente acceso
//! exclusivo de escritura a todo el worktree. Los ficheros son la fuente de
//! verdad; SQLite (`ea-store`) guarda el historial y el diario.

mod bus;
mod candado;
pub mod config;
mod error;
mod fs_atomico;
mod rutina;
pub mod skills;
mod tipos;

pub use bus::{Bus, EntradaCola, ENV_ESTADO, raiz_estado};
pub use candado::{CandadoWorktree, Ocupante, listar_semaforos, listar_semaforos_en};
pub use config::ConfigOrquestador;
pub use skills::{bloque_prompt, ConfigSkills, Skill};
pub use rutina::{slug_valido, validar_entradas, CabeceraRutina, Paso, Rutina, MAX_ENTRADAS, MAX_VALOR_ENTRADA};
pub use error::{ErrorBus, Resultado};
pub use tipos::{
    Agente, Estado, EstadoTrabajo, Evento, Fase, FaseInforme, Informe, Modo, Orden, ResultadoInforme, RutinaRef, Topes, transicion,
};
