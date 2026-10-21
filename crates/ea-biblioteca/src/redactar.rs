//! Redacción: lo que sale de la biblioteca (CLI, web, MCP, diario) nunca
//! lleva secretos (RF-003). De un MCP solo se conservan los NOMBRES de `env`
//! y de las cabeceras, de la URL solo esquema, host y puerto, y los
//! argumentos y comandos pasan por [`redactar_texto`] y [`redactar_args`].

use serde_json::Value;

const MASCARA: &str = "***";

/// ¿El nombre de una clave, variable o cabecera suena a secreto?
pub fn nombre_secreto(nombre: &str) -> bool {
    let n = nombre.to_ascii_lowercase();
    ["token", "key", "secret", "pass", "auth", "bearer", "cookie", "credential", "signature", "session"]
        .iter()
        .any(|s| n.contains(s))
}

/// Enmascara el valor de todo `clave=valor` o `clave:valor` (también dentro de
/// JSON, de cadenas de conexión con `;` y de cabeceras) cuya clave suene a
/// secreto. Enfoque conservador: ante la duda, se oculta.
fn enmascarar_pares(texto: &str) -> String {
    let c: Vec<char> = texto.chars().collect();
    let n = c.len();
    let es_fin = |ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '&' | '}' | ']' | '"' | '\'');
    let mut rangos: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        let sep = c[i] == '=' || (c[i] == ':' && !(c.get(i + 1) == Some(&'/') && c.get(i + 2) == Some(&'/')));
        if !sep {
            i += 1;
            continue;
        }
        let mut k_fin = i;
        if k_fin > 0 && matches!(c[k_fin - 1], '"' | '\'') {
            k_fin -= 1;
        }
        let mut k_ini = k_fin;
        while k_ini > 0 && (c[k_ini - 1].is_alphanumeric() || matches!(c[k_ini - 1], '_' | '-' | '.')) {
            k_ini -= 1;
        }
        let clave: String = c[k_ini..k_fin].iter().collect();
        if clave.is_empty() || !nombre_secreto(&clave) {
            i += 1;
            continue;
        }
        let mut v = i + 1;
        while v < n && (c[v] == ' ' || c[v] == '\t') {
            v += 1;
        }
        let (ini, mut fin) = if v < n && matches!(c[v], '"' | '\'') {
            let q = c[v];
            let mut f = v + 1;
            while f < n && c[f] != q {
                f += if c[f] == '\\' { 2 } else { 1 };
            }
            (v + 1, f.min(n))
        } else {
            let mut f = v;
            while f < n && !es_fin(c[f]) {
                f += 1;
            }
            (v, f)
        };
        if fin > ini {
            // `Authorization: Bearer xxx`: el esquema y la credencial.
            let palabra: String = c[ini..fin].iter().collect::<String>().to_ascii_lowercase();
            if matches!(palabra.as_str(), "bearer" | "basic" | "token") {
                let mut f = fin;
                while f < n && c[f] == ' ' {
                    f += 1;
                }
                while f < n && !es_fin(c[f]) {
                    f += 1;
                }
                fin = f;
            }
            rangos.push((ini, fin));
            i = fin;
        } else {
            i += 1;
        }
    }
    let mut sal = String::with_capacity(texto.len());
    let mut pos = 0;
    for (a, b) in rangos {
        sal.extend(&c[pos..a]);
        sal.push_str(MASCARA);
        pos = b;
    }
    sal.extend(&c[pos..]);
    sal
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
    redactar_palabras(&enmascarar_pares(texto))
}

fn redactar_palabras(texto: &str) -> String {
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
            matches!(o, "-H" | "--header" | "-e" | "--env" | "-E" | "-p" | "-k" | "-t") || (o.starts_with('-') && nombre_secreto(o))
        };
        if a.starts_with('-') && !a.contains('=') && opcion_con_valor(a) {
            salida.push(a.clone());
            ocultar_siguiente = true;
        } else if es_estructura(a) && nombre_secreto(a) {
            // Un JSON, objeto o lista entero que menciona algo secreto.
            salida.push(MASCARA.into());
        } else {
            salida.push(redactar_texto_sin_normalizar(a));
        }
    }
    salida
}

/// Como [`redactar_texto`], pero un argumento sin espacios queda intacto en
/// su forma (no se parte ni se reune).
fn redactar_texto_sin_normalizar(a: &str) -> String {
    let a = enmascarar_pares(a);
    if a.chars().any(char::is_whitespace) { redactar_palabras(&a) } else { redactar_palabra(&a) }
}

/// ¿Es el argumento un objeto o una lista (`{…}`/`[…]`) entero?
fn es_estructura(a: &str) -> bool {
    let t = a.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
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

    fn args(v: &[&str]) -> Vec<String> {
        redactar_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn json_en_un_argumento_no_filtra() {
        let a = args(&["-c", r#"{"Authorization":"Bearer abc123"}"#, "--cfg", r#"{"a": 1, "apiKey": "zzz9"}"#, "--ok", r#"{"a": 1}"#]);
        assert_eq!(a[1], "***");
        assert_eq!(a[3], "***");
        assert_eq!(a[5], r#"{"a": 1}"#);
        let t = redactar_texto(r#"run {"token": 12345, "x": "y"}"#);
        assert!(!t.contains("12345"), "{t}");
        let t = redactar_texto(r#"run {"Authorization":"Bearer abc123"}"#);
        assert!(!t.contains("abc123"), "{t}");
    }

    #[test]
    fn cadenas_de_conexion_y_pares_con_punto_y_coma() {
        let a = args(&["--dsn", "Server=x;Password=abc;Db=d", "--otra", "host=h; pwd=zz; session_id=s9"]);
        assert!(!a[1].contains("abc") && a[1].contains("Server=x") && a[1].contains("Db=d"), "{a:?}");
        assert!(!a[3].contains("s9"), "{a:?}");
        assert!(!redactar_texto("a:b Cookie: sid=qq9").contains("qq9"));
    }

    #[test]
    fn opciones_cortas_con_valor() {
        let a = args(&["-p", "hunter2", "-k", "kk1", "-t", "tt2", "-y", "paquete"]);
        assert_eq!(a, ["-p", "***", "-k", "***", "-t", "***", "-y", "paquete"]);
        let a = args(&["--session-id", "abc", "--x-credential", "d", "--cookie=e"]);
        assert_eq!(a, ["--session-id", "***", "--x-credential", "***", "--cookie=***"]);
    }

    #[test]
    fn nombres_sin_valores() {
        let v = serde_json::json!({"B": "x", "A": "y"});
        assert_eq!(nombres_de(Some(&v)), ["A", "B"]);
        assert!(nombres_de(None).is_empty());
    }
}
