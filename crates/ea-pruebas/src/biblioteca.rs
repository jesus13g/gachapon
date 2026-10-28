//! El HOME pequeño de la biblioteca que comparten las pruebas de la CLI y
//! del MCP: una skill con un uso y dos MCP con secretos.

use std::path::Path;

use crate::escribe;

/// La skill del HOME, con un uso en las transcripciones.
pub const SKILL: &str = "claude-code/skill/usuario/propia";
/// El MCP con secretos en su entorno.
pub const MCP: &str = "claude-code/mcp/usuario/con-secretos";
/// Lo que no puede salir nunca en una respuesta.
pub const SECRETOS: &[&str] = &["SECRETO_ENV", "SECRETO_HEADER"];

/// Escribe en `home` la skill `propia`, `.claude.json` con `con-secretos`
/// (stdio, secreto en el entorno) y `remoto` (http, secreto en la cabecera),
/// y una transcripción con un uso de la skill.
pub fn home(home: &Path) {
    escribe(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    escribe(
        home.join(".claude.json"),
        r#"{"mcpServers":{"con-secretos":{"type":"stdio","command":"npx","args":["-y","pkg"],"env":{"API_KEY":"SECRETO_ENV"}},
            "remoto":{"type":"http","url":"https://m.ejemplo.com/mcp","headers":{"Authorization":"Bearer SECRETO_HEADER"}}}}"#,
    );
    escribe(
        home.join(".claude/projects/-p/s1.jsonl"),
        concat!(
            r#"{"type":"assistant","timestamp":"2026-10-01T10:00:00Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Skill","input":{"skill":"propia"}}]}}"#,
            "\n"
        ),
    );
}
