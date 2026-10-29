//! El contrato de errores de los cuerpos (U-10): en toda ruta que lee un
//! cuerpo, el JSON roto, el tipo de contenido que no es JSON, un campo que
//! sobra y un tipo que no encaja son un 400 con `{"error": "…"}` en JSON,
//! nunca el texto plano ni el 415/422 de axum. Y nada se escribe.

use crate::comun::*;

/// Todas las rutas que leen un cuerpo (con la clave del panel: aquí no se
/// prueba la guardia, sino lo que viene después).
const CON_CUERPO: [&str; 31] = [
    "/api/diario/anotar",
    "/api/rutinas/validar",
    "/api/rutinas/registrar",
    "/api/rutinas/revision-diaria/lanzar",
    "/api/rutinas/revision-diaria/previa",
    "/api/trabajos",
    "/api/skills/probar",
    "/api/trabajos/x/reformular",
    "/api/trabajos/x/confirmar",
    "/api/equipos",
    "/api/propuestas/1/decidir",
    "/api/acciones/simular",
    "/api/acciones/ejecutar",
    "/api/terminal/abrir",
    "/api/boveda/promover",
    "/api/boveda/guardar",
    "/api/boveda/crear",
    "/api/boveda/renombrar",
    "/api/boveda/eliminar",
    "/api/boveda/carpeta",
    "/api/boveda/carpeta/renombrar",
    "/api/boveda/carpeta/eliminar",
    "/api/boveda/mostrar",
    "/api/boveda/papelera/restaurar",
    "/api/memoria/-home-u-proy/crear",
    "/api/memoria/-home-u-proy/refactor.md",
    "/api/memoria/-home-u-proy/refactor.md/eliminar",
    "/api/memoria/-home-u-proy/refactor.md/renombrar",
    "/api/ajustes",
    "/api/biblioteca/simular",
    "/api/biblioteca/ejecutar",
];

/// Un POST con la clave, el cuerpo tal cual y el tipo de contenido que se
/// diga (`None`: sin cabecera). Devuelve el código, el tipo y el cuerpo.
async fn crudo(f: &Fixture, ruta: &str, tipo: Option<&str>, cuerpo: &str) -> (StatusCode, String, String) {
    let mut req = Request::builder().method(Method::POST).uri(ruta).header("host", "localhost:8092").header("x-ea-clave", f.state.clave.texto());
    if let Some(t) = tipo {
        req = req.header("content-type", t);
    }
    let resp = f.app.clone().oneshot(req.body(Body::from(cuerpo.to_string())).unwrap()).await.unwrap();
    let status = resp.status();
    let ct = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, ct, String::from_utf8_lossy(&bytes).into_owned())
}

/// 400, `application/json` y `{"error": "<no vacío>"}` que contiene `pista`.
fn es_error_400(caso: &str, (s, ct, t): &(StatusCode, String, String), pista: &str) {
    assert_eq!(*s, StatusCode::BAD_REQUEST, "{caso}: {t}");
    assert!(ct.starts_with("application/json"), "{caso}: tipo {ct}");
    let v: Value = serde_json::from_str(t).unwrap_or_else(|_| panic!("{caso}: no es JSON: {t}"));
    let e = v["error"].as_str().unwrap_or_else(|| panic!("{caso}: sin error: {t}"));
    assert!(!e.is_empty() && e.contains(pista), "{caso}: «{e}» no dice «{pista}»");
    assert_eq!(v.as_object().map(|o| o.len()), Some(1), "{caso}: solo `error`: {t}");
}

#[tokio::test]
async fn un_cuerpo_invalido_es_siempre_400_con_error_en_json() {
    let f = montar();
    let antes = f.state.store.lock().await.list_events(&Default::default()).unwrap().len();
    for ruta in CON_CUERPO {
        for roto in ["{", "{\"a\": }", "[1, 2", "no es json", "\u{0}"] {
            es_error_400(&format!("{ruta} con {roto:?}"), &crudo(&f, ruta, Some("application/json"), roto).await, "cuerpo inválido");
        }
        // JSON válido pero con un campo que ninguna ruta admite.
        es_error_400(&format!("{ruta} con campo de más"), &crudo(&f, ruta, Some("application/json"), r#"{"sobra": 1}"#).await, "sobra");
        // El tipo de contenido no es JSON (o no viene), aunque el cuerpo lo sea.
        for tipo in [Some("text/plain"), Some("application/x-www-form-urlencoded"), None] {
            es_error_400(&format!("{ruta} como {tipo:?}"), &crudo(&f, ruta, tipo, "{}").await, "application/json");
        }
    }
    // Ninguno escribió nada: ni trabajos, ni notas, ni eventos.
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t, json!([]));
    assert_eq!(f.state.store.lock().await.list_events(&Default::default()).unwrap().len(), antes);
}

#[tokio::test]
async fn un_tipo_que_no_encaja_es_400_con_error_en_json() {
    let f = montar();
    let casos = [
        ("/api/diario/anotar", r#"{"texto": 3}"#),
        ("/api/rutinas/revision-diaria/lanzar", r#"{"entradas": {"x": 1}}"#),
        ("/api/rutinas/validar", r#"{"toml": ["no"]}"#),
        ("/api/trabajos", r#"{"titulo": "x", "worktree": "/tmp", "objetivo": "x", "topes": {"iteraciones": -1}}"#),
        ("/api/trabajos/x/reformular", r#"{"nota": "x", "fase": "dos"}"#),
        ("/api/equipos", r#"{"titulo": "x", "objetivo": "x", "worktree": "/tmp", "todos": "sí"}"#),
        ("/api/propuestas/1/decidir", r#"{"decision": true}"#),
        ("/api/acciones/simular", r#"{"herramienta": 5}"#),
        ("/api/boveda/guardar", r#"{"ruta": "a.md", "contenido": null, "version": ""}"#),
        ("/api/ajustes", r#"{"version": "", "cambios": []}"#),
        // Un objeto JSON y nada más: ni listas, ni textos, ni `null`.
        ("/api/terminal/abrir", r#""ea propuestas decidir 1""#),
        ("/api/memoria/-home-u-proy/crear", "null"),
    ];
    for (ruta, cuerpo) in casos {
        es_error_400(&format!("{ruta} con {cuerpo}"), &crudo(&f, ruta, Some("application/json"), cuerpo).await, "cuerpo inválido");
    }
    // Lo que distingue a reformular de confirmar: cada una solo su campo.
    let (s, r) = post(&f, "/api/trabajos/x/reformular", json!({ "contexto": "x" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    let (s, r) = post(&f, "/api/trabajos/x/confirmar", json!({ "nota": "x" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
}

#[tokio::test]
async fn un_cuerpo_vacio_cuenta_como_un_objeto_vacio() {
    let f = montar();
    // Donde todo es opcional, vale: lo que pide la página sin cuerpo.
    let (s, _, t) = crudo(&f, "/api/rutinas/revision-diaria/previa", Some("application/json"), "  \n").await;
    assert_eq!(s, StatusCode::OK, "{t}");
    // Donde algo es obligatorio, el error dice qué falta.
    es_error_400("anotar vacío", &crudo(&f, "/api/diario/anotar", Some("application/json"), "").await, "texto");
}
