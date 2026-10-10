//! Capa de datos compartida del ecosistema de asistente.
//!
//! Único punto de acceso a la base de datos embebida (tareas y diario de
//! eventos). Expone solo tipos de dominio y la fachada [`Store`], nunca
//! tipos de `rusqlite`, para que futuros consumidores (proceso orquestador,
//! servidor MCP) puedan reutilizar este crate sin acoplarse a la librería
//! de acceso concreta.

mod connection;
mod error;
mod migrations;
mod models;
mod repository;
mod store;

pub use connection::resolve_db_path;
pub use error::{StoreError, StoreResult};
pub use models::{
    EstadoPropuesta, Event, EventOutcome, NewEvent, NewTask, Propuesta, RoutineRun, RunOutcome,
    Task, TaskOrigin, TaskStatus,
};
pub use repository::events::EventFilter;
pub use repository::propuestas::PropuestaFilter;
pub use repository::tasks::TaskFilter;
pub use store::Store;
