//! Lectores de solo lectura, uno por agente. Cada uno es una función pura
//! sobre [`crate::Raices`]: lo que no existe es vacío y lo que no se puede
//! interpretar deja un aviso, nunca un error.

pub mod antigravity;
pub mod claude;
pub mod codex;
pub(crate) mod comun;
pub mod ecosistema;
pub mod opencode;
pub mod qwen;
