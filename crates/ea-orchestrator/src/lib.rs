//! Ejecutor de rutinas del ecosistema de asistente (§9.2, §10, §11.2, §12.3
//! de la especificación): sondea la cola de tareas por encargos que un
//! disparador `systemd --user timer` encoló, ejecuta el grafo de pasos
//! declarativo de la rutina correspondiente contra `ea-proxy` (decisiones)
//! y `ea-mcp` (acciones de solo lectura), y encola una propuesta por cada
//! punto donde la rutina "querría" mutar en vez de ejecutar nada. El mismo
//! sondeo ejecuta las tareas dirigidas al modelo local (`tareas.rs`).

pub mod activacion;
pub mod config;
pub mod ejecutor;
pub mod http;
pub mod mcp_client;
pub mod plantilla;
pub mod proxy_client;
pub mod rutina;
pub mod tareas;
pub mod turno;
pub mod voz;
