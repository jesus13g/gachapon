//! Codex y Antigravity: solo última actividad.
//!
//! Codex guarda sus hilos en `~/.codex/state_5.sqlite` (`threads.rollout_path`,
//! `updated_at_ms`) y los rollouts en `~/.codex/sessions/`; en esta máquina no
//! hay ninguno, así que no se ha podido verificar el formato de las llamadas a
//! herramientas y no se extraen: solo la fecha. Antigravity solo deja
//! `~/.gemini/antigravity-cli/log/cli-*.log`: vale su `mtime`.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

pub fn actividad_codex(estado: &Path, sesiones: &Path) -> Option<i64> {
    let de_db = Connection::open_with_flags(estado, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .ok()
        .and_then(|c| c.query_row("SELECT max(updated_at_ms) FROM threads", [], |f| f.get::<_, Option<i64>>(0)).ok().flatten());
    de_db.max(super::mtime_maximo(sesiones, |n| n.starts_with("rollout-") && n.ends_with(".jsonl")))
}

pub fn actividad_antigravity(logs: &Path) -> Option<i64> {
    super::mtime_maximo(logs, |n| n.starts_with("cli-") && n.ends_with(".log"))
}
