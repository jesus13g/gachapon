//! Redacción: lo que sale de la biblioteca (CLI, web, MCP, diario) nunca
//! lleva secretos (RF-003). De un MCP solo se conservan los NOMBRES de `env`
//! y de las cabeceras, de la URL solo esquema, host y puerto, y los
//! argumentos y comandos pasan por [`redactar_texto`] y [`redactar_args`].

use serde_json::Value;

const MASCARA: &str = "***";

/// ¿El nombre de una clave, variable o cabecera suena a secreto?
pub fn nombre_secreto(nombre: &str) -> bool {
    let n = nombre.to_ascii_lowercase();
    ["token", "key", "secret", "pass", "auth", "bearer", "cookie", "credential", "signature"]
        .iter()
        .any(|s| n.contains(s))
}

/// `esquema://host[:puerto]` de una URL: sin credenciales, ruta, consulta ni
/// fragmento. Si no se reconoce como URL con esquema, `***`.
pub fn redactar_url(url: &str) -> String {
    let url = url.trim();
    let Some((esquema, resto)) = url.split_once("://") else {
        return MASCARA.into();
    };
    if esquema.is_empty() || !esquema.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')) {
        return MASCARA.into();
    }
    let autoridad = resto.split(['/', '?', '#']).next().unwrap_or("");
    // Lo que va antes de la última «@» son credenciales.
    let host = autoridad.rsplit_once('@').map_or(autoridad, |(_, h)| h);
    if host.is_empty() || host.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return MASCARA.into();
    }
    format!("{esquema}://{host}")
}

/// ¿Parece una credencial por su forma (prefijos conocidos, JWT, cadenas
/// largas opacas)?
fn forma_de_secreto(p: &str) -> bool {
    let prefijos = ["sk-", "sk_", "ghp_", "gho_", "ghs_", "github_pat_", "xox", "AKIA", "AIza", "eyJ", "glpat-", "npm_", "pk_", "rk_"];
    if prefijos.iter().any(|x| p.starts_with(x)) && p.len() >= 12 {
        return true;
    }
    p.len() >= 32
        && p.bytes().any(|b| b.is_ascii_digit())
        && p.bytes().any(|b| b.is_ascii_alphabetic())
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'='))
}

/// Redacta un trozo de texto libre palabra a palabra: URL a esquema+host,
/// `CLAVE=valor` de nombre secreto, valores tras `Bearer`/`Basic`, y cadenas
/// con forma de credencial. El espaciado se normaliza a un espacio.
pub fn redactar_texto(texto: &str) -> String {
    let mut salida: Vec<String> = Vec::new();
    let mut tras_esquema = false;
    for palabra in texto.split_whitespace() {
        if tras_esquema {
            tras_esquema = false;
            salida.push(MASCARA.into());
            continue;
        }
        let l = palabra.to_ascii_lowercase();
        if l == "bearer" || l == "basic" || l == "token" {
            salida.push(palabra.into());
            tras_esquema = true;
            continue;
        }
        salida.push(redactar_palabra(palabra));
    }
    salida.join(" ")
}

fn redactar_palabra(p: &str) -> String {
    // Comillas alrededor: se conservan.
    let (ini, med, fin) = {
        let t = p.trim_start_matches(['"', '\'']);
        let ini = &p[..p.len() - t.len()];
        let m = t.trim_end_matches(['"', '\'', ',', ';']);
        (ini, m, &t[m.len()..])
    };
    let cuerpo = if med.contains("://") {
        redactar_url(med)
    } else if let Some((k, _)) = med.split_once('=') {
        let k_limpia = k.trim_start_matches('-');
        if nombre_secreto(k_limpia) || forma_de_secreto(med.split_once('=').map_or("", |x| x.1)) {
            format!("{k}={MASCARA}")
        } else {
            med.to_string()
        }
    } else if forma_de_secreto(med) {
        MASCARA.into()
    } else {
        med.to_string()
    };
    format!("{ini}{cuerpo}{fin}")
}

/// Redacta los argumentos de un comando: el valor de una opción de nombre
/// secreto (`--token x`, `--api-key=x`, `-H "Authorization: …"`, `-e K=V`) y
/// cualquier palabra con forma de credencial o URL.
pub fn redactar_args(args: &[String]) -> Vec<String> {
    let mut salida = Vec::with_capacity(args.len());
    let mut ocultar_siguiente = false;
    for a in args {
        if ocultar_siguiente {
            ocultar_siguiente = false;
            salida.push(MASCARA.into());
            continue;
        }
        let opcion_con_valor = |o: &str| {
            matches!(o, "-H" | "--header" | "-e" | "--env" | "-E") || (o.starts_with('-') && nombre_secreto(o))
        };
        if a.starts_with('-') && !a.contains('=') && opcion_con_valor(a) {
            salida.push(a.clone());
            ocultar_siguiente = true;
        } else {
            salida.push(redactar_texto_sin_normalizar(a));
        }
    }
    salida
}

/// Como [`redactar_texto`], pero un argumento sin espacios queda intacto en
/// su forma (no se parte ni se reune).
fn redactar_texto_sin_normalizar(a: &str) -> String {
    if a.chars().any(char::is_whitespace) { redactar_texto(a) } else { redactar_palabra(a) }
}

/// Los nombres (nunca los valores) de un objeto JSON, ordenados.
pub fn nombres_de(v: Option<&Value>) -> Vec<String> {
    let mut n: Vec<String> = v.and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    n.sort();
    n
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn la_url_se_queda_en_esquema_host_y_puerto() {
        assert_eq!(redactar_url("https://user:pw@api.ejemplo.com:8443/v1/mcp/TOKEN123?key=abc#f"), "https://api.ejemplo.com:8443");
        assert_eq!(redactar_url("http://127.0.0.1:8765/mcp"), "http://127.0.0.1:8765");
        assert_eq!(redactar_url("sin esquema"), "***");
        assert_eq!(redactar_url("https://@/x"), "***");
    }

    #[test]
    fn texto_y_argumentos() {
        let t = redactar_texto("curl -H Authorization: Bearer abc123 https://h.com/p?t=1 TOKEN=zzz ok");
        for fuga in ["abc123", "/p?t=1", "zzz"] {
            assert!(!t.contains(fuga), "{t}");
        }
        assert!(t.contains("https://h.com") && t.contains("ok"), "{t}");

        let a = redactar_args(&[
            "--token".into(),
            "s3cr3t".into(),
            "--api-key=otro".into(),
            "-e".into(),
            "K=V".into(),
            "-y".into(),
            "@scope/paquete".into(),
            "sk-abcdefghijklmnop".into(),
            "https://x.com/a?b=c".into(),
        ]);
        assert_eq!(a, ["--token", "***", "--api-key=***", "-e", "***", "-y", "@scope/paquete", "***", "https://x.com"]);
    }

    #[test]
    fn nombres_sin_valores() {
        let v = serde_json::json!({"B": "x", "A": "y"});
        assert_eq!(nombres_de(Some(&v)), ["A", "B"]);
        assert!(nombres_de(None).is_empty());
    }
}
