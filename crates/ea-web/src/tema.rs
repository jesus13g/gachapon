//! Variables CSS a partir del tema activo de Omarchy, para que la web siga
//! al escritorio cuando cambia de tema. Cada petición relee el fichero; si
//! falta o es ilegible se usan los colores de City 783.

use std::path::Path;

/// (clave en `colors.toml`, variable CSS, valor por defecto).
const COLORES: &[(&str, &str, &str)] = &[
    ("background", "bg", "#181a1f"),
    ("lighter_bg", "bg2", "#20232a"),
    ("dark_bg", "bg0", "#101216"),
    ("selection", "sel", "#2b2f37"),
    ("fg", "fg", "#b9bec6"),
    ("bright_fg", "fg-hi", "#eceff2"),
    ("readable_muted", "fg-lo", "#8f949c"),
    ("dark_fg", "fg-dim", "#6a707a"),
    ("muted", "line", "#4b515b"),
    // Sin `accent`: el acento naranja de las marcas es fijo y lo pone `app.css`.
    ("text_accent", "hot", "#ff5c5c"),
];

fn es_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn css(path: &Path) -> String {
    let tema: Option<toml::Table> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok());

    let mut out = String::from(":root{");
    for (clave, variable, defecto) in COLORES {
        let valor = tema
            .as_ref()
            .and_then(|t| t.get(*clave))
            .and_then(|v| v.as_str())
            .filter(|v| es_color(v))
            .unwrap_or(defecto);
        out.push_str(&format!("--{variable}:{valor};"));
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_fichero_usa_los_colores_por_defecto() {
        let css = css(Path::new("/no/existe/colors.toml"));
        assert!(css.contains("--bg:#181a1f;"));
        assert!(css.contains("--hot:#ff5c5c;"));
        assert!(!css.contains("--accent"), "el acento lo fija app.css");
    }

    #[test]
    fn rechaza_valores_que_no_son_colores() {
        assert!(es_color("#ad2222"));
        assert!(!es_color("red"));
        assert!(!es_color("#ad2222;}body{display:none"));
    }
}
