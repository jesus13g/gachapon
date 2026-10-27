//! Orquestador del ecosistema de asistente: el ejecutor (`ejecutor`) toma
//! trabajos de la cola del rol ejecutor (`modelo-local`) y los lanza con el
//! agente que ocupe ese rol (`agentes`: Qwen Code, Codex u OpenCode). Desde la
//! spec 019 hay un trabajador por ejecutor despierto (`trabajadores`).

pub mod agentes;
pub mod ejecutor;
pub mod sandbox;
pub mod trabajadores;
