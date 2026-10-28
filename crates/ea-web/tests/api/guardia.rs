//! La guardia del panel: solo `localhost`, escrituras con JSON y con la
//! clave de sesión, lecturas sin ella, validación de lo que llega y los
//! estáticos con sus cabeceras de seguridad.

use crate::comun::*;

#[tokio::test]
async fn rechaza_host_ajeno() {
    let f = montar();
    for host in ["evil.example", "localhost.evil.example:8092", "192.168.1.5:8092"] {
        let (s, _) = enviar(&f, Method::GET, "/api/estado", host, None, false).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "host {host}");
    }
    for host in ["localhost", "localhost:8092", "127.0.0.1:8092"] {
        let (s, _) = enviar(&f, Method::GET, "/api/estado", host, None, false).await;
        assert_eq!(s, StatusCode::OK, "host {host}");
    }
}

#[tokio::test]
async fn las_escrituras_exigen_json() {
    let f = montar();
    let (s, _) = enviar(
        &f,
        Method::POST,
        "/api/rutinas/revision-diaria/lanzar",
        "localhost:8092",
        Some(json!({})),
        false,
    )
    .await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

/// Rutas de escritura representativas (una por zona de la API).
const ESCRITURAS: [(&str, &str); 11] = [
    ("/api/biblioteca/simular", r#"{"herramienta":"ea_desactivar_herramienta","argumentos":{"elemento":"claude-code/skill/usuario/propia"}}"#),
    ("/api/biblioteca/ejecutar", r#"{"herramienta":"ea_desactivar_herramienta","argumentos":{"elemento":"claude-code/skill/usuario/propia"},"precondicion":{}}"#),
    ("/api/rutinas/revision-diaria/lanzar", "{}"),
    ("/api/trabajos", concat!(r#"{"titulo":"x","worktree":""#, tmp!(), r#"","objetivo":"x","criterio":"true"}"#)),
    ("/api/propuestas/1/decidir", r#"{"decision":"rechazar"}"#),
    ("/api/acciones/simular", r#"{"herramienta":"ea_status","argumentos":{}}"#),
    ("/api/boveda/crear", r#"{"ruta":"compartida/x.md"}"#),
    ("/api/ajustes", r#"{"version":"","cambios":{}}"#),
    ("/api/servicios/ea-mcp/reiniciar", "{}"),
    ("/api/equipos", concat!(r#"{"titulo":"x","objetivo":"x","worktree":""#, tmp!(), r#"","todos":true}"#)),
    ("/api/equipos/x/dormir", "{}"),
];

#[tokio::test]
async fn las_escrituras_sin_clave_dan_403() {
    let f = montar();
    let id = f.state.store.lock().await.crear_propuesta(texto("x")).unwrap().id;
    assert_eq!(id, 1);
    let otra = "0".repeat(64);
    for (ruta, cuerpo) in ESCRITURAS {
        let cuerpo: Value = serde_json::from_str(cuerpo).unwrap();
        for clave in [None, Some(""), Some("mala"), Some(otra.as_str()), Some(&f.state.clave.texto()[..63])] {
            for metodo in [Method::POST, Method::PUT, Method::DELETE] {
                let (s, t) =
                    enviar_con_clave(&f, metodo.clone(), ruta, "localhost:8092", Some(cuerpo.clone()), true, clave).await;
                assert_eq!(s, StatusCode::FORBIDDEN, "{metodo} {ruta} con {clave:?}");
                assert!(t.contains("ea-web-open"), "mensaje claro: {t}");
            }
        }
    }
    // Nada ha cambiado: la propuesta sigue pendiente y no se lanzó la rutina.
    let (_, p) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(p.as_array().map(Vec::len), Some(1), "{p}");
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    // El 403 lleva la marca para que la página pase a solo lectura.
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/rutinas/revision-diaria/lanzar")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let r = f.app.clone().oneshot(req).await.unwrap();
    assert_eq!(r.headers().get("x-ea-clave").unwrap(), "requerida");
}

#[tokio::test]
async fn con_la_clave_correcta_funcionan() {
    let f = montar();
    let id = f.state.store.lock().await.crear_propuesta(texto("x")).unwrap().id;
    let clave = f.state.clave.texto().to_string();
    let (s, _) = enviar_con_clave(
        &f, Method::POST, &format!("/api/propuestas/{id}/decidir"), "localhost:8092",
        Some(json!({ "decision": "rechazar" })), true, Some(&clave),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, t) = enviar_con_clave(
        &f, Method::POST, "/api/rutinas/revision-diaria/lanzar", "localhost:8092", Some(json!({})), true, Some(&clave),
    )
    .await;
    assert!(s.is_success(), "{s} {t}");
    // La clave no exime del resto de la guardia.
    let (s, _) = enviar_con_clave(
        &f, Method::POST, "/api/rutinas/revision-diaria/lanzar", "localhost:8092", Some(json!({})), false, Some(&clave),
    )
    .await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (s, _) = enviar_con_clave(
        &f, Method::POST, "/api/rutinas/revision-diaria/lanzar", "evil.example", Some(json!({})), true, Some(&clave),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn las_lecturas_no_piden_clave_y_la_sesion_dice_si_vale() {
    let f = montar();
    for ruta in ["/", "/app.js", "/cliente.js", "/api/estado", "/api/trabajos", "/api/ajustes", "/api/propuestas"] {
        let (s, _) = enviar_con_clave(&f, Method::GET, ruta, "localhost:8092", None, false, None).await;
        assert_eq!(s, StatusCode::OK, "{ruta}");
    }
    let (s, t) = enviar_con_clave(&f, Method::GET, "/api/sesion", "localhost", None, false, None).await;
    assert_eq!((s, serde_json::from_str::<Value>(&t).unwrap()), (StatusCode::OK, json!({ "escritura": false })));
    let (_, t) = enviar_con_clave(&f, Method::GET, "/api/sesion", "localhost", None, false, Some("mala")).await;
    assert_eq!(serde_json::from_str::<Value>(&t).unwrap(), json!({ "escritura": false }));
    let clave = f.state.clave.texto().to_string();
    let (_, t) = enviar_con_clave(&f, Method::GET, "/api/sesion", "localhost", None, false, Some(&clave)).await;
    assert_eq!(serde_json::from_str::<Value>(&t).unwrap(), json!({ "escritura": true }));
}

#[tokio::test]
async fn valida_las_entradas() {
    let f = montar();

    let (s, _) = get(&f, "/api/propuestas?estado=inventado").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = get(&f, "/api/trabajos/no-existe").await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _) = get(&f, "/api/diario?desde=ayer").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

/// La respuesta entera, con sus cabeceras, de una petición sin clave ni cuerpo.
async fn respuesta(f: &Fixture, metodo: Method, ruta: &str, host: &str) -> axum::response::Response {
    let req = Request::builder().method(metodo).uri(ruta).header("host", host).body(Body::empty()).unwrap();
    f.app.clone().oneshot(req).await.unwrap()
}

/// Cada fichero de `static/` tiene su fila en `ESTATICOS` y sale con el tipo
/// de su extensión, sin caché y tal cual; lo que no está en la tabla es 404. El
/// tema trae los colores de Omarchy salvo el acento, que fija `app.css`.
#[tokio::test]
async fn sirve_la_pagina_y_el_tema() {
    let f = montar();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    let fichero = |ruta: &str| if ruta == "/" { "index.html".to_string() } else { ruta.trim_start_matches('/').to_string() };
    let mut en_disco: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    let mut en_tabla: Vec<String> = ea_web::http::ESTATICOS.iter().map(|(ruta, _, _)| fichero(ruta)).collect();
    en_disco.sort();
    en_tabla.sort();
    assert_eq!(en_disco, en_tabla, "cada fichero de static/ tiene su fila en ESTATICOS, y solo esos");
    for &(ruta, tipo, cuerpo) in ea_web::http::ESTATICOS {
        let nombre = fichero(ruta);
        let esperado = match nombre.rsplit_once('.').map(|(_, ext)| ext) {
            Some("html") => "text/html; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("js") => "text/javascript; charset=utf-8",
            otra => panic!("{ruta}: extensión sin tipo ({otra:?})"),
        };
        assert_eq!(tipo, esperado, "{ruta}");
        assert_eq!(cuerpo, std::fs::read_to_string(dir.join(&nombre)).unwrap(), "{ruta}");
        let r = respuesta(&f, Method::GET, ruta, "localhost:8092").await;
        assert_eq!(r.status(), StatusCode::OK, "{ruta}");
        assert_eq!(r.headers()["content-type"], tipo, "{ruta}");
        assert_eq!(r.headers()["cache-control"], "no-store", "{ruta}");
        assert_eq!(r.into_body().collect().await.unwrap().to_bytes(), cuerpo.as_bytes(), "{ruta}");
    }
    for ruta in ["/index.html", "/no-existe.js", "/static/app.js", "/app.js/", "/app.js.map", "/../Cargo.toml"] {
        assert_eq!(respuesta(&f, Method::GET, ruta, "localhost:8092").await.status(), StatusCode::NOT_FOUND, "{ruta}");
    }
    let (s, css) = enviar(&f, Method::GET, "/tema.css", "localhost:8092", None, false).await;
    assert_eq!(s, StatusCode::OK);
    assert!(css.starts_with(":root{") && css.contains("--bg:#181a1f;") && !css.contains("--accent"), "{css}");
}

/// S-10: la CSP, `nosniff` y `Referrer-Policy` van en todas las respuestas: la
/// página, sus ficheros, el tema, la API, un 404 y los 403 de la guardia (Host
/// ajeno, escritura sin clave). La CSP no admite guiones en línea ni marcos.
#[tokio::test]
async fn las_cabeceras_de_seguridad_van_en_todas_las_respuestas() {
    let f = montar();
    let casos = [
        (Method::GET, "/", "localhost:8092", StatusCode::OK),
        (Method::GET, "/app.js", "localhost:8092", StatusCode::OK),
        (Method::GET, "/tema.css", "127.0.0.1:8092", StatusCode::OK),
        (Method::GET, "/api/estado", "localhost:8092", StatusCode::OK),
        (Method::GET, "/no-existe", "localhost:8092", StatusCode::NOT_FOUND),
        (Method::GET, "/", "evil.example", StatusCode::FORBIDDEN),
        (Method::POST, "/api/diario/anotar", "localhost:8092", StatusCode::FORBIDDEN),
    ];
    for (metodo, ruta, host, estado) in casos {
        let r = respuesta(&f, metodo.clone(), ruta, host).await;
        assert_eq!(r.status(), estado, "{metodo} {ruta} ({host})");
        let h = r.headers();
        assert_eq!(h["content-security-policy"], ea_web::http::CSP, "{metodo} {ruta} ({host})");
        assert_eq!(h["x-content-type-options"], "nosniff", "{metodo} {ruta} ({host})");
        assert_eq!(h["referrer-policy"], "no-referrer", "{metodo} {ruta} ({host})");
    }
    let directivas: Vec<&str> = ea_web::http::CSP.split(';').map(str::trim).collect();
    for d in ["default-src 'self'", "script-src 'self'", "connect-src 'self'", "frame-ancestors 'none'", "base-uri 'none'", "object-src 'none'"] {
        assert!(directivas.contains(&d), "falta {d:?} en {directivas:?}");
    }
}
