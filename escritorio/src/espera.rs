//! Página local «esperando a los servicios» (spec 021, decisión 8, RF-008).
//! La sirve el propio shell por un protocolo propio; el vigía cambia a la
//! URL del panel en cuanto `ea-web` responde. El shell no arranca servicios.

use url::Url;

/// Nombre del protocolo propio.
pub const PROTOCOLO: &str = "ea-espera";

/// La página, embebida.
pub const HTML: &str = include_str!("espera.html");

/// El párrafo de ayuda de la página (cómo ver y cargar los servicios).
const AYUDA: &str = "<p id=\"ayuda\">Para ver el estado de los servicios: <code>ea servicios ver</code>. Para cargarlos: <code>ea servicios instalar</code>.</p>";

/// La página que se sirve. En modo WSL (spec 023) la ayuda nombra la
/// distribución: los servicios corren dentro de ella con `systemd --user`.
pub fn html(wsl: Option<&str>) -> String {
    match wsl {
        None => HTML.to_string(),
        Some(d) => {
            // El nombre ya viene validado ([A-Za-z0-9._-]); nada que escapar.
            let ayuda = format!(
                "<p id=\"ayuda\">Los servicios corren en WSL (<code>{d}</code>). Para ver su estado: <code>wsl -d {d} -- systemctl --user status ea-web</code>. Si no están instalados: <code>wsl -d {d} -- ~/.local/bin/ea servicios instalar</code>.</p>"
            );
            HTML.replace(AYUDA, &ayuda)
        }
    }
}

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
    fn en_modo_wsl_la_ayuda_nombra_la_distribucion() {
        assert_eq!(html(None), HTML);
        assert!(HTML.contains(AYUDA), "el párrafo de ayuda sigue en la página");
        let h = html(Some("Ubuntu"));
        assert!(h.contains("wsl -d Ubuntu -- systemctl --user status ea-web"));
        assert!(!h.contains("<code>ea servicios ver</code>"));
        assert!(h.contains("id=\"motivo\""));
    }

    #[test]
    fn el_motivo_va_escapado() {
        let g = guion_motivo("a\"</script>");
        assert!(!g.contains("</script>"));
        assert!(g.contains("textContent"));
    }
}
