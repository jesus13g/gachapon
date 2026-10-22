//! Lo mínimo de XML de property list que necesitan los LaunchAgents de
//! launchd. Puro: solo construye texto.

/// Escapa un texto para ir dentro de un elemento XML.
pub(crate) fn escapar(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            c => s.push(c),
        }
    }
    s
}

/// `<key>k</key><string>v</string>` con la sangría dada.
pub(crate) fn cadena(sangria: &str, clave: &str, valor: &str) -> String {
    format!("{sangria}<key>{}</key>\n{sangria}<string>{}</string>\n", escapar(clave), escapar(valor))
}

pub(crate) fn entero(sangria: &str, clave: &str, valor: i64) -> String {
    format!("{sangria}<key>{}</key>\n{sangria}<integer>{valor}</integer>\n", escapar(clave))
}

pub(crate) fn booleano(sangria: &str, clave: &str, valor: bool) -> String {
    format!("{sangria}<key>{}</key>\n{sangria}<{valor}/>\n", escapar(clave))
}

/// `<key>ProgramArguments</key><array>…</array>`.
pub(crate) fn lista_de_cadenas(sangria: &str, clave: &str, valores: &[String]) -> String {
    let mut s = format!("{sangria}<key>{}</key>\n{sangria}<array>\n", escapar(clave));
    for v in valores {
        s.push_str(&format!("{sangria}  <string>{}</string>\n", escapar(v)));
    }
    s.push_str(&format!("{sangria}</array>\n"));
    s
}

/// El documento entero alrededor del cuerpo de la `<dict>` raíz.
pub(crate) fn documento(cuerpo: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n{cuerpo}</dict>\n</plist>\n"
    )
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn escapa_lo_que_rompe_el_xml() {
        assert_eq!(escapar("a&b<c>\"d'"), "a&amp;b&lt;c&gt;&quot;d&apos;");
        assert!(cadena("  ", "Label", "x<y").contains("<string>x&lt;y</string>"));
        assert!(booleano("", "RunAtLoad", true).contains("<true/>"));
    }
}
