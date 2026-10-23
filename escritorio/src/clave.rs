//! La clave de sesión del panel (spec 021, decisión 4, RF-004). El shell la
//! lee del mismo fichero que `ea-web-open` y la deja en
//! `sessionStorage['ea-clave']` con un guion de inicio: nunca va en la URL,
//! ni en argv, ni en el registro.

use std::path::Path;

/// Una clave válida son 64 caracteres hexadecimales (lo que escribe `ea-web`).
pub fn es_valida(clave: &str) -> bool {
    clave.len() == 64 && clave.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Lee la clave del fichero. `None` si falta, no se puede leer o no es
/// válida: el panel abre entonces en solo lectura.
pub fn leer(fichero: &Path) -> Option<String> {
    let texto = std::fs::read_to_string(fichero).ok()?;
    let clave = texto.trim();
    es_valida(clave).then(|| clave.to_string())
}

/// Un literal JavaScript seguro (comillas, barras y saltos de línea
/// escapados; `<` y los separadores de línea Unicode también, por si el
/// guion acabara dentro de una etiqueta).
fn literal(texto: &str) -> String {
    let json = serde_json::to_string(texto).expect("una cadena siempre se serializa");
    json.replace('<', "\\u003c").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
}

/// Guion de inicio: deja la clave en `sessionStorage` solo si aún no hay
/// ninguna (una reentrega posterior ya la renovó y un guion viejo no debe
/// pisarla al recargar). Sin clave no hace nada.
pub fn guion_inicio(clave: Option<&str>) -> String {
    match clave {
        Some(c) if es_valida(c) => format!(
            "try {{ if (!sessionStorage.getItem('ea-clave')) sessionStorage.setItem('ea-clave', {}); }} catch (e) {{}}",
            literal(c)
        ),
        _ => String::new(),
    }
}

/// Guion de renovación: pone (o quita, si ya no hay clave válida) la clave y
/// recarga la vista actual; el hash se conserva porque `reload` no lo toca.
pub fn guion_renovar(clave: Option<&str>) -> String {
    let guardar = match clave {
        Some(c) if es_valida(c) => format!("sessionStorage.setItem('ea-clave', {})", literal(c)),
        _ => "sessionStorage.removeItem('ea-clave')".to_string(),
    };
    format!("try {{ {guardar}; }} catch (e) {{}} location.reload();")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUENA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn solo_vale_una_clave_de_64_hex() {
        assert!(es_valida(BUENA));
        assert!(!es_valida(""));
        assert!(!es_valida(&BUENA[..63]));
        assert!(!es_valida(&format!("{}z", &BUENA[..63])));
        assert!(!es_valida(&format!("{BUENA}0")));
    }

    #[test]
    fn el_guion_lleva_la_clave_como_literal_y_solo_ahi() {
        let g = guion_inicio(Some(BUENA));
        assert!(g.contains(&format!("\"{BUENA}\"")));
        assert!(g.contains("sessionStorage.setItem('ea-clave'"));
        assert_eq!(g.matches(BUENA).count(), 1);
        assert!(!g.contains("location"), "no toca la URL");
    }

    #[test]
    fn una_clave_invalida_no_entra_en_ningun_guion() {
        let mala = "\"); alert(1); (\"";
        assert_eq!(guion_inicio(Some(mala)), "");
        assert!(!guion_renovar(Some(mala)).contains("alert"));
        assert!(guion_renovar(Some(mala)).contains("removeItem"));
        assert_eq!(guion_inicio(None), "");
    }

    #[test]
    fn el_literal_escapa_lo_peligroso() {
        let l = literal("a\"b\\c\n</script>\u{2028}");
        assert!(!l.contains('\n'));
        assert!(!l.contains("</"));
        assert!(!l.contains('\u{2028}'));
        assert!(l.starts_with('"') && l.ends_with('"'));
        assert!(l.contains("\\\""));
        assert!(l.contains("\\\\"));
    }

    #[test]
    fn renovar_recarga_y_no_pisa_el_hash() {
        let g = guion_renovar(Some(BUENA));
        assert!(g.contains("location.reload()"));
        assert!(!g.contains("location.hash") && !g.contains("location.href") && !g.contains("location.replace"));
    }

    #[test]
    fn leer_del_fichero() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("clave");
        assert_eq!(leer(&f), None);
        std::fs::write(&f, format!("{BUENA}\n")).unwrap();
        assert_eq!(leer(&f).as_deref(), Some(BUENA));
        std::fs::write(&f, "basura").unwrap();
        assert_eq!(leer(&f), None);
    }
}
