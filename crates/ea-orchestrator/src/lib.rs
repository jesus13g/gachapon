//! Orquestador del ecosistema de asistente. En reconstrucción hacia el
//! modelo Claude Code + Qwen Code (ver `orquestador-claude-qwen.md`): el
//! ejecutor (`ejecutor`) toma trabajos de la cola `modelo-local` y los
//! lanza con Qwen Code headless.

pub mod config;
pub mod ejecutor;
