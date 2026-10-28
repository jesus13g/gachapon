//! Las pruebas en JavaScript de la página (`tests/js/`), lanzadas con `node`.
//! Sin `node` en el PATH se saltan con un aviso (el panel no lo necesita para
//! funcionar, solo estas pruebas), salvo con `EA_EXIGIR_NODE=1` (el CI): ahí
//! fallan.

use std::path::Path;
use std::process::Command;

fn node() -> Option<Command> {
    match Command::new("node").arg("--version").output() {
        Ok(s) if s.status.success() => Some(Command::new("node")),
        _ => {
            assert!(std::env::var_os("EA_EXIGIR_NODE").is_none_or(|v| v != "1"), "EA_EXIGIR_NODE=1 y no hay `node` en el PATH");
            eprintln!("aviso: no hay `node` en el PATH; se saltan las pruebas de JavaScript");
            None
        }
    }
}

/// `node --test tests/js/<fichero>`.
fn pasa(fichero: &str) {
    let Some(mut node) = node() else { return };
    let prueba = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/js").join(fichero);
    let salida = node.arg("--test").arg(&prueba).output().expect("lanzar node");
    assert!(
        salida.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&salida.stdout),
        String::from_utf8_lossy(&salida.stderr)
    );
}

#[test]
fn el_parseo_del_hash_pasa_sus_pruebas() {
    pasa("enlaces.test.js");
}

/// El Markdown que pinta lo que escriben los agentes no deja pasar HTML
/// activo (`static/markdown.js`).
#[test]
fn el_markdown_pasa_sus_pruebas_de_xss() {
    pasa("markdown.test.js");
}

/// Los `.js` de `static/`, por nombre.
fn guiones() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    let mut v: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".js"))
        .map(|n| {
            let texto = std::fs::read_to_string(dir.join(&n)).unwrap();
            (n, texto)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn los_scripts_de_la_pagina_son_javascript_valido() {
    let static_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    let nombres: Vec<String> = guiones().into_iter().map(|(n, _)| n).collect();
    for js in ["tema-inicial.js", "gashapon.js", "markdown.js"] {
        assert!(nombres.iter().any(|n| n == js), "{js}: {nombres:?}");
    }
    for js in nombres {
        let Some(mut node) = node() else { return };
        let salida = node.arg("--check").arg(static_dir.join(&js)).output().expect("lanzar node");
        assert!(salida.status.success(), "{js}: {}", String::from_utf8_lossy(&salida.stderr));
    }
}

/// Los `on<evento>=` de un texto: un espacio, `on`, letras e `=` (un atributo
/// de manejador en HTML, también dentro de una plantilla de JS).
fn manejadores_en_linea(texto: &str) -> Vec<String> {
    let b = texto.as_bytes();
    let mut v = Vec::new();
    for (i, _) in texto.match_indices("on") {
        if i == 0 || !b[i - 1].is_ascii_whitespace() {
            continue;
        }
        let letras = b[i + 2..].iter().take_while(|c| c.is_ascii_alphabetic()).count();
        if letras > 0 && b.get(i + 2 + letras) == Some(&b'=') {
            v.push(texto[i..i + 3 + letras].to_string());
        }
    }
    v
}

/// `palabra(` sin una letra, cifra, `_` o `.` delante (`eval(` sí, `revalida(` o `x.eval(` no).
fn llamadas(texto: &str, palabra: &str) -> usize {
    texto
        .match_indices(&format!("{palabra}("))
        .filter(|(i, _)| *i == 0 || !matches!(texto.as_bytes()[i - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'$'))
        .count()
}

/// S-10: la CSP del panel (`script-src 'self'`) no admite guiones en línea, ni
/// manejadores `on…=`, ni `javascript:`, ni `eval`. Si alguien los añade, esto
/// falla antes de que el navegador los bloquee en silencio.
#[test]
fn la_pagina_no_necesita_guiones_en_linea_ni_eval() {
    let index = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("static/index.html")).unwrap();
    for (i, _) in index.match_indices("<script") {
        let etiqueta = &index[i..i + index[i..].find('>').unwrap() + 1];
        assert!(etiqueta.contains(" src=\"/"), "guion en línea en index.html: {etiqueta}");
        assert!(index[i + etiqueta.len()..].starts_with("</script>"), "guion con cuerpo en index.html: {etiqueta}");
    }
    assert_eq!(manejadores_en_linea(&index), Vec::<String>::new(), "index.html");
    assert!(!index.contains("javascript:"), "index.html");
    for (nombre, texto) in guiones() {
        assert_eq!(manejadores_en_linea(&texto), Vec::<String>::new(), "{nombre}");
        for palabra in ["eval", "Function"] {
            assert_eq!(llamadas(&texto, palabra), 0, "{nombre}: {palabra}(");
        }
        assert!(!texto.contains("new Function"), "{nombre}");
        for temporizador in ["setTimeout", "setInterval"] {
            for comilla in ['\'', '"', '`'] {
                assert!(!texto.contains(&format!("{temporizador}({comilla}")), "{nombre}: {temporizador} con texto");
            }
        }
    }
    // El detector ve lo que tiene que ver.
    assert_eq!(manejadores_en_linea("<b onclick=\"x()\" class=a>"), ["onclick="]);
    assert_eq!(manejadores_en_linea("an.onfinish = f; button=1"), Vec::<String>::new());
    assert_eq!((llamadas("eval(x)", "eval"), llamadas("revalida(x)", "eval")), (1, 0));
}
