//! Página local «esperando a los servicios» (spec 021, decisión 8, RF-008).
//! La sirve el propio shell por un protocolo propio; el vigía cambia a la
//! URL del panel en cuanto `ea-web` responde. El shell no arranca servicios.

use url::Url;

/// Nombre del protocolo propio.
pub const PROTOCOLO: &str = "ea-espera";

/// La página, embebida.
pub const HTML: &str = include_str!("espera.html");

/// URL de la página de espera según el sistema (en Windows, WebView2 sirve
/// los protocolos propios como `http://<protocolo>.localhost/`).
pub fn url_espera() -> String {
    if cfg!(windows) { format!("http://{PROTOCOLO}.localhost/") } else { format!("{PROTOCOLO}://localhost/") }
}

/// ¿Es la URL de la página de espera?
pub fn es_url_espera(url: &Url) -> bool {
    match url.scheme() {
        s if s == PROTOCOLO => true,
        "http" => url.host_str() == Some(&format!("{PROTOCOLO}.localhost")),
        _ => false,
    }
}

/// Guion que escribe el motivo de la espera en la página.
pub fn guion_motivo(motivo: &str) -> String {
    let literal = serde_json::to_string(motivo).expect("una cadena siempre se serializa").replace('<', "\\u003c");
    format!("var m = document.getElementById('motivo'); if (m) m.textContent = {literal};")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconoce_su_url_y_no_otras() {
        assert!(es_url_espera(&Url::parse(&url_espera()).unwrap()));
        assert!(es_url_espera(&Url::parse("ea-espera://localhost/").unwrap()));
        assert!(es_url_espera(&Url::parse("http://ea-espera.localhost/").unwrap()));
        assert!(!es_url_espera(&Url::parse("http://example.com/").unwrap()));
        assert!(!es_url_espera(&Url::parse("https://ea-espera.localhost/").unwrap()));
    }

    #[test]
    fn la_pagina_nombra_como_arrancar_los_servicios() {
        assert!(HTML.contains("id=\"motivo\""));
        assert!(HTML.contains("ea servicios"));
        assert!(!HTML.contains("<script src"), "sin recursos externos");
    }

    #[test]
    fn el_motivo_va_escapado() {
        let g = guion_motivo("a\"</script>");
        assert!(!g.contains("</script>"));
        assert!(g.contains("textContent"));
    }
}
