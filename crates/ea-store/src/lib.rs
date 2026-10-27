//! Capa de datos compartida del ecosistema de asistente.
//!
//! Único punto de acceso a la base de datos embebida (diario de eventos,
//! propuestas, índice de la bóveda e historial de trabajos). Expone solo tipos de dominio y la fachada [`Store`], nunca
//! tipos de `rusqlite`, para que sus consumidores (CLI, orquestador, MCP, web)
//! no se acoplen a la librería de acceso concreta.

mod connection;
mod error;
mod migrations;
mod models;
mod orquestador;
mod repository;
mod store;

pub use connection::resolve_db_path;
pub use error::{StoreError, StoreResult};
pub use models::{
    AccionPropuesta, EjecucionPropuesta, EntradaIndice, ResultadoBusqueda, RutaIndexada, EstadoEjecucion, EstadoPropuesta, Event, EventOutcome, NewEvent, NuevaPropuesta, Propuesta,
};
pub use orquestador::{ultimo_latido, ultimo_sandbox, EstadoOrquestador, SandboxInfo, ResumenAgente, AGENTES};
pub use repository::events::EventFilter;
pub use repository::notas::{NuevaNota, ACCION_NOTA, ACTOR_NOTA, MAX_TEXTO_NOTA};
pub use repository::propuestas::PropuestaFilter;
pub use repository::trabajos::{FilaFase, FilaRutina, FilaTrabajo};
pub use store::Store;
