//! Qué orígenes puede cargar la ventana (spec 021, decisión 3, RF-002).
//! Solo el panel local, en `127.0.0.1` o `localhost` y en el puerto exacto de
//! `red.puerto_web`; todo lo demás se abre con el navegador del sistema.

use url::Url;

/// La URL del panel en el puerto dado, con un fragmento opcional (ya validado).
pub fn url_panel(puerto: u16, fragmento: Option<&str>) -> String {
    match fragmento {
        Some(f) if !f.is_empty() => format!("http://127.0.0.1:{puerto}/#{f}"),
        _ => format!("http://127.0.0.1:{puerto}/"),
    }
}

/// ¿Puede la ventana navegar a `url`? Esquema `http`, host `127.0.0.1` o
/// `localhost` y puerto exacto. Sin credenciales en la URL.
pub fn permitido(url: &Url, puerto: u16) -> bool {
    url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
        && url.port() == Some(puerto)
        && url.username().is_empty()
        && url.password().is_none()
}

/// Decisión para `on_navigation`: la ventana también puede mostrar su propia
/// página de espera (protocolo propio) y la página en blanco inicial.
pub fn navegacion_permitida(url: &Url, puerto: u16) -> bool {
    permitido(url, puerto) || crate::espera::es_url_espera(url) || url.as_str() == "about:blank"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn acepta_el_panel_local() {
        assert!(permitido(&u("http://127.0.0.1:8092/"), 8092));
        assert!(permitido(&u("http://localhost:8092/api/estado"), 8092));
        assert!(permitido(&u("http://127.0.0.1:8092/#trabajos/abc"), 8092));
    }

    #[test]
    fn rechaza_otros_puertos_hosts_y_esquemas() {
        assert!(!permitido(&u("http://127.0.0.1:8093/"), 8092));
        assert!(!permitido(&u("http://127.0.0.1/"), 8092));
        assert!(!permitido(&u("https://127.0.0.1:8092/"), 8092));
        assert!(!permitido(&u("http://example.com:8092/"), 8092));
        assert!(!permitido(&u("https://github.com/jesus13g/gachapon"), 8092));
        assert!(!permitido(&u("http://127.0.0.1.evil.com:8092/"), 8092));
        assert!(!permitido(&u("http://evil.com@127.0.0.1:8092/"), 8092));
        assert!(!permitido(&u("http://[::1]:8092/"), 8092));
        assert!(!permitido(&u("file:///etc/passwd"), 8092));
        assert!(!permitido(&u("javascript:alert(1)"), 8092));
    }

    #[test]
    fn la_navegacion_admite_la_espera_y_lo_vacio() {
        assert!(navegacion_permitida(&u("about:blank"), 8092));
        assert!(navegacion_permitida(&u(&crate::espera::url_espera()), 8092));
        assert!(!navegacion_permitida(&u("http://example.com/"), 8092));
    }

    #[test]
    fn la_url_del_panel_lleva_el_fragmento() {
        assert_eq!(url_panel(8092, None), "http://127.0.0.1:8092/");
        assert_eq!(url_panel(8092, Some("trabajos/x")), "http://127.0.0.1:8092/#trabajos/x");
    }
}
