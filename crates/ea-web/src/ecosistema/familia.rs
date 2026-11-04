//! La familia visual de un agente: qué variante de máquina le toca en la
//! escena. La deciden la ficha del agente del catálogo
//! (`ea_ajustes::FichaAgente::familia`) y, si el agente no se conoce, esta
//! tabla por modelo; el navegador solo traduce cada familia a colores y
//! detalles (`VARIANTES` en `gashapon.js`).
//!
//! Para un proveedor nuevo: su familia en la ficha (o una fila aquí, por un
//! trozo del nombre del modelo) y su entrada en `VARIANTES`. Lo que no casa
//! con nada es `local` (si su modelo es local) o `desconocida`.

/// Por un trozo del nombre del modelo, en minúsculas, cuando el agente no se
/// conoce. El orden importa: la primera que casa gana.
const POR_MODELO: &[(&str, &str)] = &[
    ("claude", "claude"),
    ("opus", "claude"),
    ("sonnet", "claude"),
    ("haiku", "claude"),
    ("fable", "claude"),
    ("qwen", "qwen"),
    ("codex", "codex"),
    ("gpt", "codex"),
    ("gemini", "antigravity"),
];

pub const DESCONOCIDA: &str = "desconocida";

pub fn familia(agente: Option<&str>, modelo: Option<&str>, local: bool) -> &'static str {
    if let Some(f) = agente.and_then(ea_ajustes::ficha) {
        return f.familia;
    }
    let modelo = modelo.unwrap_or_default().to_lowercase();
    if let Some(f) = POR_MODELO.iter().find(|(trozo, _)| modelo.contains(trozo)) {
        return f.1;
    }
    if local { "local" } else { DESCONOCIDA }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_agente_manda_sobre_el_modelo() {
        assert_eq!(familia(Some("claude-code"), Some("haiku"), false), "claude");
        // OpenCode con un Qwen local sigue siendo una máquina de OpenCode.
        assert_eq!(familia(Some("opencode"), Some("qwen3-coder-30b"), true), "opencode");
        assert_eq!(familia(Some("qwen-code"), None, true), "qwen");
    }

    #[test]
    fn cada_agente_del_catalogo_tiene_su_familia() {
        let familias = ["claude-code", "qwen-code", "codex", "opencode", "antigravity"].map(|a| familia(Some(a), Some("llama-4"), true));
        assert_eq!(familias, ["claude", "qwen", "codex", "opencode", "antigravity"]);
        assert_eq!(familia(Some("Codex"), Some("llama-4"), true), "local", "el id se compara tal cual");
    }

    #[test]
    fn sin_agente_conocido_decide_el_modelo_y_si_no_el_fallback() {
        assert_eq!(familia(Some("otro"), Some("Claude-Sonnet-5"), false), "claude");
        assert_eq!(familia(None, Some("gemini-3-pro"), false), "antigravity");
        assert_eq!(familia(None, Some("llama-4"), true), "local");
        assert_eq!(familia(None, Some("llama-4"), false), DESCONOCIDA);
        assert_eq!(familia(None, None, false), DESCONOCIDA);
    }
}
