//! Orquestador del ecosistema de asistente: el ejecutor (`ejecutor`) toma
//! trabajos de la cola del rol ejecutor (`modelo-local`) y los lanza con el
//! agente que ocupe ese rol (`agentes`: Qwen Code, Codex u OpenCode).

pub mod agentes;
pub mod config;
pub mod ejecutor;
