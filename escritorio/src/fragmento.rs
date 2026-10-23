//! Enlaces profundos (spec 021, decisión 6, RF-006): el fragmento que llega
//! en los argumentos de un arranque (`ea-escritorio '#trabajos/<id>'`).

/// Longitud máxima de un fragmento aceptado.
const MAX: usize = 200;

fn caracter_valido(c: char) -> bool {
    c.is_ascii_alphanumeric() || "/_?=&.-".contains(c)
}

/// El fragmento de un argumento suelto: acepta `#x` y `x`; `None` si está
/// vacío, es demasiado largo o trae algo fuera de `[A-Za-z0-9/_?=&.-]`.
/// Devuelve el fragmento sin la almohadilla.
pub fn analizar(arg: &str) -> Option<String> {
    let f = arg.strip_prefix('#').unwrap_or(arg);
    (!f.is_empty() && f.len() <= MAX && f.chars().all(caracter_valido)).then(|| f.to_string())
}

/// El fragmento de la línea de órdenes completa (el primer elemento es el
/// programa). Ignora las opciones (`--oculto`) y toma el primer argumento
/// válido.
pub fn de_argumentos<S: AsRef<str>>(argv: &[S]) -> Option<String> {
    argv.iter().skip(1).map(AsRef::as_ref).filter(|a| !a.starts_with("--")).find_map(analizar)
}

/// Guion que lleva la vista de la página a ese fragmento (literal JSON).
pub fn guion_ir_a(fragmento: &str) -> String {
    let literal = serde_json::to_string(&format!("#{fragmento}")).expect("una cadena siempre se serializa");
    format!("location.hash = {literal};")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acepta_con_y_sin_almohadilla() {
        assert_eq!(analizar("#trabajos/abc-1").as_deref(), Some("trabajos/abc-1"));
        assert_eq!(analizar("diario?latidos=1").as_deref(), Some("diario?latidos=1"));
    }

    #[test]
    fn rechaza_lo_raro() {
        assert_eq!(analizar(""), None);
        assert_eq!(analizar("#"), None);
        assert_eq!(analizar("#a b"), None);
        assert_eq!(analizar("#a';alert(1)//"), None);
        assert_eq!(analizar("#a<b"), None);
        assert_eq!(analizar("#a%20b"), None);
        assert_eq!(analizar(&format!("#{}", "a".repeat(201))), None);
    }

    #[test]
    fn de_argumentos_salta_el_programa_y_las_opciones() {
        assert_eq!(de_argumentos(&["ea-escritorio"]), None);
        assert_eq!(de_argumentos(&["ea-escritorio", "#sistema"]).as_deref(), Some("sistema"));
        assert_eq!(de_argumentos(&["ea-escritorio", "--oculto", "#trabajos/x"]).as_deref(), Some("trabajos/x"));
        assert_eq!(de_argumentos(&["ea-escritorio", "mal que va", "#diario"]).as_deref(), Some("diario"));
        assert_eq!(de_argumentos(&["trabajos/x"]), None, "el primero es el programa");
    }

    #[test]
    fn el_guion_usa_un_literal_json() {
        assert_eq!(guion_ir_a("trabajos/x"), "location.hash = \"#trabajos/x\";");
    }
}
