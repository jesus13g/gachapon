//! Capa de datos compartida del ecosistema de asistente.
//!
//! Único punto de acceso a la base de datos embebida (diario de eventos,
//! propuestas, índice de la bóveda e historial de trabajos). Expone solo tipos de dominio y la fachada [`Store`], nunca
//! tipos de `rusqlite`, para que sus consumidores (CLI, orquestador, MCP, web)
//! no se acoplen a la librería de acceso concreta.

mod connection;
mod error;
mod filtro;
mod migrations;
mod models;
mod orquestador;
mod repository;
mod store;

pub use error::{StoreError, StoreResult};
pub use filtro::{ahora_rfc3339, fecha_flexible, prefijo_accion_valido, rfc3339, ErrorFiltro, FiltroDiario};
pub use models::{
    AccionPropuesta, EjecucionPropuesta, EntradaIndice, ResultadoBusqueda, RutaIndexada, EstadoEjecucion, EstadoPropuesta, Event, EventOutcome, NewEvent, NuevaPropuesta, Propuesta,
};
pub use orquestador::{ultimo_latido, ultimo_sandbox, EstadoOrquestador, SandboxInfo, ResumenAgente, AGENTES};
pub use repository::events::EventFilter;
pub use repository::notas::{NuevaNota, ACCION_NOTA, ACTOR_NOTA, MAX_TEXTO_NOTA};
pub use repository::propuestas::PropuestaFilter;
pub use repository::trabajos::{FilaFase, FilaRutina, FilaTrabajo};
pub use store::Store;
