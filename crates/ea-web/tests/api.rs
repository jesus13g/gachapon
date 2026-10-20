use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use ea_store::{EventOutcome, NewEvent, Store};
use ea_web::{api, AppState, WebConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;

const RUTINA_EJEMPLO: &str = "descripcion = \"Revisión diaria de ejemplo\"
";

struct Fixture {
    state: Arc<AppState>,
    app: Router,
    _dir: TempDir,
}

fn texto(descripcion: &str) -> ea_store::NuevaPropuesta {
    ea_store::NuevaPropuesta { proponente: "claude-code".into(), descripcion: descripcion.into(), accion: None }
}

const RUTINA_BUS: &str = "[rutina]\nslug = \"revision-diaria\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"test -f {salida}\"\n[entradas]\nsalida = \"informe.md\"\n[[pasos]]\ntexto = \"revisa y escribe {salida}\"\n";

/// Las pruebas que lanzan rutinas comparten la variable de entorno del
/// directorio de estado del bus: una a una, cada cual con el suyo.
static BUS_EXCLUSIVO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn aislar_bus(f: &Fixture) -> tokio::sync::MutexGuard<'static, ()> {
    let g = BUS_EXCLUSIVO.lock().await;
    unsafe { std::env::set_var("EA_ESTADO_DIR", f._dir.path().join("estado")) };
    g
}

fn montar() -> Fixture {
    // Nunca el estado real del bus: un directorio temporal para todo el proceso
    // de pruebas (las que lanzan trabajos lo cambian con `aislar_bus`).
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    ESTADO.get_or_init(|| {
        let d = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("EA_ESTADO_DIR", d.path()) };
        d
    });
    let dir = tempfile::tempdir().expect("directorio temporal");
    let rutinas_dir = dir.path().join("rutinas");
    std::fs::create_dir_all(&rutinas_dir).unwrap();
    std::fs::write(rutinas_dir.join("revision-diaria.toml"), RUTINA_EJEMPLO).unwrap();

    let store = Store::open_at(&dir.path().join("test.db")).expect("abrir store");
    store
        .guardar_rutina(&ea_store::FilaRutina {
            slug: "revision-diaria".into(),
            version: 1,
            definicion: RUTINA_BUS.into(),
            creada_en: "2026-01-01T00:00:00Z".into(),
            creada_por: "test".into(),
        })
        .unwrap();
    let config = WebConfig {
        puerto: 0,
        umbral_heartbeat_s: 60,
        tema_path: dir.path().join("no-existe.toml"),
        rutinas_dir,
        boveda: ea_boveda::Boveda::en(dir.path().join("boveda")),
        systemctl: false,
        sesiones_dir: Some(dir.path().join("sesiones")),
        subagentes_dir: Some(dir.path().join("subagentes")),
        proyectos_claude_dir: Some(dir.path().join("proyectos")),
        estado_dir: dir.path().join("estado"),
        ajustes: ea_ajustes::Fuentes::aislada(dir.path()),
    };
    let state = Arc::new(AppState::new(store, config).expect("estado"));
    Fixture { app: api::router(state.clone()), state, _dir: dir }
}

/// Como el panel abierto con `ea-web-open`: con la clave de sesión del
/// estado de prueba (que nunca se escribe en el `$XDG_RUNTIME_DIR` real).
async fn enviar(
    f: &Fixture,
    metodo: Method,
    ruta: &str,
    host: &str,
    cuerpo: Option<Value>,
    json_ct: bool,
) -> (StatusCode, String) {
    let clave = f.state.clave.texto().to_string();
    enviar_con_clave(f, metodo, ruta, host, cuerpo, json_ct, Some(&clave)).await
}

async fn enviar_con_clave(
    f: &Fixture,
    metodo: Method,
    ruta: &str,
    host: &str,
    cuerpo: Option<Value>,
    json_ct: bool,
    clave: Option<&str>,
) -> (StatusCode, String) {
    let mut req = Request::builder().method(metodo).uri(ruta).header("host", host);
    if let Some(c) = clave {
        req = req.header("x-ea-clave", c);
    }
    if json_ct {
        req = req.header("content-type", "application/json");
    }
    let body = cuerpo.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty);
    let resp = f.app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn get(f: &Fixture, ruta: &str) -> (StatusCode, Value) {
    let (s, t) = enviar(f, Method::GET, ruta, "localhost:8092", None, false).await;
    (s, serde_json::from_str(&t).unwrap_or(Value::Null))
}

async fn post(f: &Fixture, ruta: &str, cuerpo: Value) -> (StatusCode, Value) {
    let (s, t) = enviar(f, Method::POST, ruta, "localhost:8092", Some(cuerpo), true).await;
    (s, serde_json::from_str(&t).unwrap_or(Value::Null))
}

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
const ESCRITURAS: [(&str, &str); 6] = [
    ("/api/rutinas/revision-diaria/lanzar", "{}"),
    ("/api/propuestas/1/decidir", r#"{"decision":"rechazar"}"#),
    ("/api/acciones/simular", r#"{"herramienta":"ea_status","argumentos":{}}"#),
    ("/api/boveda/crear", r#"{"ruta":"compartida/x.md"}"#),
    ("/api/ajustes", r#"{"version":"","cambios":{}}"#),
    ("/api/servicios/ea-mcp/reiniciar", "{}"),
];

#[tokio::test]
async fn las_escrituras_sin_clave_dan_403() {
    let f = montar();
    let _g = aislar_bus(&f).await;
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
    let _g = aislar_bus(&f).await;
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
async fn el_estado_resume_trabajos_y_agentes_y_el_sistema_antiguo_no_existe() {
    let f = montar();
    let _g = aislar_bus(&f).await;

    let (s, e) = get(&f, "/api/estado").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(e["trabajos"]["en_cola"], 0);
    assert_eq!(e["trabajos"]["cerrado"], 0);
    assert!(e.get("tareas").is_none());
    assert_eq!(e["agentes"][0]["nombre"], "claude-code");
    assert!(e["agentes"][0]["ultimo"].is_null());

    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["trabajos"]["en_cola"], 1);

    for ruta in ["/api/tareas", "/api/tareas/1"] {
        let (s, _) = get(&f, ruta).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{ruta}");
    }
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

#[tokio::test]
async fn el_diario_oculta_los_latidos_salvo_que_se_pidan() {
    let f = montar();
    {
        let store = f.state.store.lock().await;
        for accion in ["heartbeat", "heartbeat", "trabajo_creado"] {
            store
                .append_event(NewEvent {
                    actor: "orquestador".into(),
                    action: accion.into(),
                    trabajo: if accion == "heartbeat" { None } else { Some("m-1".into()) },
                    outcome: Some(EventOutcome::Ok),
                    duration_ms: None,
                    detail: None,
                })
                .unwrap();
        }
    }

    let (_, sin) = get(&f, "/api/diario").await;
    assert_eq!(sin.as_array().unwrap().len(), 1);
    assert_eq!(sin[0]["accion"], "trabajo_creado");
    assert_eq!(sin[0]["trabajo"], "m-1");

    let (_, del_trabajo) = get(&f, "/api/diario?latidos=1&trabajo=m-1").await;
    assert_eq!(del_trabajo.as_array().unwrap().len(), 1);

    let (_, con) = get(&f, "/api/diario?latidos=1").await;
    assert_eq!(con.as_array().unwrap().len(), 3);

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["orquestador"]["vivo"], true);
}

fn acciones(d: &Value) -> Vec<String> {
    let mut v: Vec<String> = d.as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

#[tokio::test]
async fn el_diario_filtra_por_actor_resultado_y_familia() {
    let f = montar();
    {
        let store = f.state.store.lock().await;
        for (actor, accion, ok) in [
            ("modelo-local", "mcp_leer_nota", true),
            ("modelo-local", "mcp_escribir_nota_denegada", false),
            ("usuario", "propuesta_decidida", false),
            ("usuario", "bus_crear", true),
            ("orquestador", "bus_entregar", false),
        ] {
            store
                .append_event(NewEvent {
                    actor: actor.into(),
                    action: accion.into(),
                    trabajo: None,
                    outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
                    duration_ms: None,
                    detail: None,
                })
                .unwrap();
        }
    }

    let (_, d) = get(&f, "/api/diario?actor=modelo-local").await;
    assert_eq!(acciones(&d), ["mcp_escribir_nota_denegada", "mcp_leer_nota"]);

    let (_, d) = get(&f, "/api/diario?resultado=error").await;
    assert_eq!(acciones(&d), ["bus_entregar", "mcp_escribir_nota_denegada", "propuesta_decidida"]);

    let (_, d) = get(&f, "/api/diario?accion=bus_").await;
    assert_eq!(acciones(&d), ["bus_crear", "bus_entregar"]);

    let (_, d) = get(&f, "/api/diario?actor=modelo-local&resultado=error&accion=mcp_").await;
    assert_eq!(acciones(&d), ["mcp_escribir_nota_denegada"]);

    // Vacíos, como los manda la barra sin filtro: no filtran.
    let (_, d) = get(&f, "/api/diario?actor=&resultado=&accion=").await;
    assert_eq!(d.as_array().unwrap().len(), 5);

    // Las transiciones del bus quedan en la columna `trabajo`: el filtro por
    // trabajo (el que usa la ficha) las encuentra.
    let _g = aislar_bus(&f).await;
    let (s, l) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    let id = l["trabajo"].as_str().unwrap();
    let (_, d) = get(&f, &format!("/api/diario?trabajo={id}&accion=bus_")).await;
    assert_eq!(acciones(&d), ["bus_crear"]);
}

#[tokio::test]
async fn el_diario_rechaza_un_resultado_o_una_accion_no_validos() {
    let f = montar();
    for q in ["accion=bus%25", "accion=BUS_", "accion=bus-crear", "accion=x%27%20OR%201", "resultado=quizas"] {
        let (s, _) = get(&f, &format!("/api/diario?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
    let (s, _) = get(&f, "/api/diario?accion=propuesta_&resultado=ok").await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn decidir_una_propuesta() {
    let f = montar();
    let id = {
        let store = f.state.store.lock().await;
        store.crear_propuesta(texto("Revisar incidencias")).unwrap().id
    };

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["propuestas_pendientes"], 1);

    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista[0]["descripcion"], "Revisar incidencias");

    let ruta = format!("/api/propuestas/{id}/decidir");
    let (s, _) = post(&f, &ruta, json!({ "decision": "quizá" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, p) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!((s, p["estado"].as_str()), (StatusCode::OK, Some("aceptada")));

    let (s, _) = post(&f, &ruta, json!({ "decision": "rechazar" })).await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["propuestas_pendientes"], 0);

    let (_, diario) = get(&f, "/api/diario").await;
    assert_eq!(diario[0]["accion"], "propuesta_decidida");
    assert_eq!(diario[0]["actor"], "usuario");
}

#[tokio::test]
async fn rutinas_listar_ver_y_lanzar() {
    let f = montar();
    let _g = aislar_bus(&f).await;

    let (_, lista) = get(&f, "/api/rutinas").await;
    assert_eq!(lista[0]["slug"], "revision-diaria");
    assert_eq!(lista[0]["version"], 1);

    let (s, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
    assert!(r["contenido"].as_str().unwrap().contains("revisa"));

    assert_eq!(lista[0]["entradas"]["salida"], "informe.md");
    let trabajos = || ea_bus::Bus::abrir().unwrap().listar_trabajos().unwrap().len();

    // Vista previa: lo sustituido, sin crear trabajo.
    let (s, p) = post(&f, "/api/rutinas/revision-diaria/previa", json!({ "entradas": { "salida": "otro.md" } })).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(p["criterio"], "test -f otro.md");
    assert!(p["objetivo"].as_str().unwrap().contains("revisa y escribe otro.md"));
    assert_eq!(p["version"], 1);
    let (s, p) = post(&f, "/api/rutinas/revision-diaria/previa", json!({})).await;
    assert_eq!((s, p["criterio"].as_str()), (StatusCode::OK, Some("test -f informe.md")));
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/previa", json!({ "entradas": { "noexiste": "1" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/previa", json!({ "entradas": { "salida": "a\nrm x" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "sin saltos de línea");
    let (s, _) = post(&f, "/api/rutinas/no-existe/previa", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // La vista previa pasa por la guardia de la clave como toda escritura.
    let (s, _) = enviar_con_clave(&f, Method::POST, "/api/rutinas/revision-diaria/previa", "localhost:8092", Some(json!({})), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(trabajos(), 0, "la vista previa no crea trabajo");

    // Una entrada no declarada: 400 y ningún trabajo.
    let (s, e) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({ "entradas": { "noexiste": "1" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(e["error"].as_str().unwrap().contains("noexiste"));
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({ "otra": 1 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "campo desconocido en el cuerpo");
    assert_eq!(trabajos(), 0);

    let (s, t) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({ "entradas": { "salida": "otro.md" } })).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(t["rutina"], "revision-diaria");
    let id = t["trabajo"].as_str().unwrap().to_string();
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(j["estado"]["estado"], "en_cola");
    let orden = ea_bus::Bus::abrir().unwrap().leer_orden(&id).unwrap();
    assert_eq!(orden.criterio_exito.as_deref(), Some("test -f otro.md"));
    assert!(orden.objetivo.contains("revisa y escribe otro.md"));
    assert!(!orden.objetivo.contains("informe.md"));
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "ya hay una ejecución en curso");
    // Cuerpo vacío: los valores por defecto (lo que ya hacía la página).
    ea_bus::Bus::abrir().unwrap().aplicar(&id, ea_bus::Evento::Cancelar).unwrap();
    let (s, t) = enviar(&f, Method::POST, "/api/rutinas/revision-diaria/lanzar", "localhost:8092", None, true).await;
    assert_eq!(s, StatusCode::CREATED, "{t}");
    let id: String = serde_json::from_str::<Value>(&t).unwrap()["trabajo"].as_str().unwrap().into();
    assert_eq!(ea_bus::Bus::abrir().unwrap().leer_orden(&id).unwrap().criterio_exito.as_deref(), Some("test -f informe.md"));

    let (s, _) = get(&f, "/api/rutinas/no-existe").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post(&f, "/api/rutinas/no-existe/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/a.b").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/rutinas/a.b/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

fn orden(modo: ea_bus::Modo, titulo: &str, worktree: &str) -> ea_bus::Orden {
    ea_bus::Orden {
        id: String::new(),
        modo,
        titulo: titulo.into(),
        worktree: worktree.into(),
        objetivo: "hacer algo".into(),
        criterio_exito: Some("true".into()),
        fases: vec![],
        revisar_cada: None,
        topes: Default::default(),
        rutina: None,
        notas_revision: vec![],
        skills: vec![],
        creada_en: String::new(),
        creada_por: "test".into(),
    }
}

#[tokio::test]
async fn trabajos_colas_y_semaforos_de_solo_lectura() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let bus = ea_bus::Bus::abrir().unwrap();

    let (s, v) = get(&f, "/api/trabajos").await;
    assert_eq!((s, v.as_array().unwrap().len()), (StatusCode::OK, 0));

    let id = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "Primera", "/tmp")).unwrap();
    let (_, v) = get(&f, "/api/trabajos").await;
    assert_eq!(v[0]["id"], id.as_str());
    assert_eq!(v[0]["modo"], "mision");
    assert_eq!(v[0]["estado"], "en_cola");
    assert!(v[0]["resultado"].is_null());
    assert!(v[0]["worktree"].as_str().unwrap().ends_with("tmp"));

    let (_, c) = get(&f, "/api/colas").await;
    assert_eq!(c["modelo-local"][0]["trabajo"], id.as_str());
    assert_eq!(c["modelo-local"][0]["titulo"], "Primera");
    assert!(c["claude-code"].as_array().unwrap().is_empty());

    let (s, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(j["orden"]["titulo"], "Primera");
    assert_eq!(j["historial"][0]["evento"], "crear");
    assert!(j["informe"].is_null());

    let (s, _) = get(&f, "/api/trabajos/no-existe").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, &format!("/api/trabajos/{id}/diff")).await;
    assert_eq!(s, StatusCode::CONFLICT, "sin informe no hay diff");

    let (s, sem) = get(&f, "/api/semaforos").await;
    assert_eq!((s, sem.as_array().unwrap().len()), (StatusCode::OK, 0));
    let _candado = ea_bus::CandadoWorktree::adquirir_en(
        bus.raiz(),
        std::path::Path::new("/tmp"),
        ea_bus::Ocupante { agente: "modelo-local".into(), trabajo: id.clone(), pid: 0, desde: "2026-10-14T10:00:00Z".into() },
        std::time::Duration::from_millis(50),
    )
    .unwrap();
    let (_, sem) = get(&f, "/api/semaforos").await;
    assert_eq!(sem[0]["trabajo"], id.as_str());
    assert_eq!(sem[0]["agente"], "modelo-local");
}

#[tokio::test]
async fn acciones_del_usuario_sobre_los_trabajos() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let bus = ea_bus::Bus::abrir().unwrap();
    let entrega = |id: &str, completa: bool| {
        bus.aplicar(id, ea_bus::Evento::Tomar).unwrap();
        let mut inf = ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Terminado);
        inf.obra_completa = completa;
        bus.aplicar(id, ea_bus::Evento::Entregar(inf)).unwrap();
    };

    // cerrar
    let a = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "A", "/tmp")).unwrap();
    let (s, _) = post(&f, &format!("/api/trabajos/{a}/cerrar"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "en cola no se cierra");
    entrega(&a, true);
    let (_, c) = get(&f, "/api/colas").await;
    assert_eq!(c["claude-code"][0]["trabajo"], a.as_str());
    let (s, r) = post(&f, &format!("/api/trabajos/{a}/cerrar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("cerrado")));
    let (_, c) = get(&f, "/api/colas").await;
    assert!(c["claude-code"].as_array().unwrap().is_empty());

    // reformular exige nota y vuelve a la cola del modelo local
    let b = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "B", "/tmp")).unwrap();
    entrega(&b, true);
    let (s, _) = post(&f, &format!("/api/trabajos/{b}/reformular"), json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = post(&f, &format!("/api/trabajos/{b}/reformular"), json!({ "nota": "falta el caso vacío" })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    assert_eq!(bus.leer_orden(&b).unwrap().notas_revision, vec!["falta el caso vacío"]);

    // continuar y cancelar
    let c1 = bus.crear_trabajo(orden(ea_bus::Modo::Obra, "C", "/tmp")).unwrap();
    entrega(&c1, false);
    let (s, r) = post(&f, &format!("/api/trabajos/{c1}/continuar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    let (s, r) = post(&f, &format!("/api/trabajos/{c1}/cancelar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("cancelado")));

    // confirmar el paquete de corrección de una rutina en espera
    let r1 = bus.crear_trabajo(orden(ea_bus::Modo::Rutina, "R", "/tmp")).unwrap();
    let (s, _) = post(&f, &format!("/api/trabajos/{r1}/confirmar"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "solo en espera");
    bus.aplicar(&r1, ea_bus::Evento::Tomar).unwrap();
    bus.aplicar(&r1, ea_bus::Evento::PonerEnEspera).unwrap();
    std::fs::write(bus.raiz().join("trabajos").join(&r1).join("paquete-correccion.md"), "# Paquete\n").unwrap();
    let (_, j) = get(&f, &format!("/api/trabajos/{r1}")).await;
    assert!(j["paquete"].as_str().unwrap().contains("Paquete"));
    let (s, r) = post(&f, &format!("/api/trabajos/{r1}/confirmar"), json!({ "contexto": "ojo con la ruta" })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_revision")));
    let paquete = std::fs::read_to_string(bus.raiz().join("trabajos").join(&r1).join("paquete-correccion.md")).unwrap();
    assert!(paquete.contains("ojo con la ruta"));
    let (_, c) = get(&f, "/api/colas").await;
    assert_eq!(c["claude-code"][0]["trabajo"], r1.as_str());
}

#[tokio::test]
async fn servicios_degradan_con_limpieza() {
    let f = montar();

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["servicios"].as_array().unwrap().len(), 3);
    assert_eq!(e["servicios"][0]["activo"], "desconocido");
    assert_eq!(e["orquestador"]["vivo"], false);

    let (s, _) = post(&f, "/api/servicios/sshd/reiniciar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post(&f, "/api/servicios/ea-mcp/reiniciar", json!({})).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn sirve_la_pagina_y_el_tema() {
    let f = montar();
    for ruta in ["/", "/app.css", "/app.js", "/cliente.js", "/enlaces.js", "/gashapon.css", "/gashapon.js", "/tour.js"] {
        let (s, _) = enviar(&f, Method::GET, ruta, "localhost:8092", None, false).await;
        assert_eq!(s, StatusCode::OK, "{ruta}");
    }
    let (s, css) = enviar(&f, Method::GET, "/tema.css", "localhost:8092", None, false).await;
    assert_eq!(s, StatusCode::OK);
    assert!(css.starts_with(":root{") && css.contains("--accent:#ad2222;"));
}

async fn proponer_accion(f: &Fixture, herramienta: &str, args: Value) -> i64 {
    let sis = ea_acciones::SoloConsulta {
        rutinas_dir: f.state.config.rutinas_dir.clone(),
        boveda: f.state.boveda.clone(),
    };
    let hechos = ea_acciones::observar(&sis, herramienta, &args).await.unwrap();
    let store = f.state.store.lock().await;
    ea_acciones::proponer(&store, &sis, &hechos, "claude-code", None, herramienta, &args)
        .unwrap()
        .0
        .id
}

#[tokio::test]
async fn aceptar_una_propuesta_con_accion_la_ejecuta_una_sola_vez() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;

    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista[0]["accion"]["herramienta"], "ea_encolar_rutina");
    assert_eq!(lista[0]["accion"]["riesgo"], "reversible");
    assert_eq!(lista[0]["proponente"], "claude-code");
    assert!(lista[0]["accion"]["simulacion"].as_str().unwrap().contains("revision-diaria"));
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert!(ts.as_array().unwrap().is_empty(), "proponer no ejecuta");

    let ruta = format!("/api/propuestas/{id}/decidir");
    let (s, p) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["estado"], "aceptada");
    assert_eq!(p["ejecutada"]["estado"], "ejecutada");
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert_eq!(ts.as_array().unwrap().len(), 1);

    let (s, _) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, d) = get(&f, "/api/diario").await;
    let ejecuciones = d.as_array().unwrap().iter().filter(|e| e["accion"] == "accion_ejecutada").count();
    assert_eq!(ejecuciones, 1);
}

#[tokio::test]
async fn una_propuesta_cuyo_estado_cambio_falla_con_mensaje_claro() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    // Mientras tanto, alguien lanza la rutina.
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);

    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["ejecutada"]["estado"], "fallida");
    assert!(p["ejecutada"]["detalle"].as_str().unwrap().contains("cambió desde la simulación"));
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert_eq!(ts.as_array().unwrap().len(), 1, "no se encoló una segunda ejecución");
}

#[tokio::test]
async fn la_web_no_confirma_irreversibles() {
    let f = montar();
    let id = proponer_accion(&f, "ea_eliminar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, lista) = get(&f, "/api/propuestas").await;
    assert_eq!(lista[0]["accion"]["riesgo"], "irreversible");

    let (s, e) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(e["error"].as_str().unwrap().contains("terminal"));
    assert!(f.state.config.rutinas_dir.join("revision-diaria.toml").exists());
    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista.as_array().unwrap().len(), 1, "sigue pendiente");

    // Rechazarla sí se puede desde la web.
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert_eq!((s, p["estado"].as_str()), (StatusCode::OK, Some("rechazada")));
    assert!(f.state.config.rutinas_dir.join("revision-diaria.toml").exists());
}

// ------------------------------------------------------------------ bóveda

fn nota(f: &Fixture, autor: &str, titulo: &str, cuerpo: &str) -> String {
    let mut s = Store::open_at(&f.state.config.rutinas_dir.parent().unwrap().join("test.db")).unwrap();
    f.state
        .boveda
        .escribir_nota(
            &mut s,
            autor,
            &ea_boveda::NuevaNota { titulo, cuerpo, ruta: None, tags: &[], origen: "manual", sobrescribir: false },
            &ea_boveda::Limites::AMPLIOS,
        )
        .unwrap()
        .ruta
}

#[tokio::test]
async fn la_boveda_se_lista_busca_lee_y_promueve_desde_la_web() {
    let f = montar();
    let ruta = nota(&f, "modelo-local", "Receta de lentejas", "Lentejas con chorizo y pimentón.");
    nota(&f, "claude-code", "Otra cosa", "Nada que ver.");

    let (s, v) = get(&f, "/api/boveda").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["notas"].as_array().unwrap().len(), 2);
    let (_, v) = get(&f, "/api/boveda?q=lentejas&region=modelo-local").await;
    assert_eq!(v["notas"].as_array().unwrap().len(), 1);
    assert!(v["notas"][0]["fragmento"].as_str().unwrap().contains("«"));
    // Una consulta a medio escribir (solo signos) no es un error.
    let (s, v) = get(&f, "/api/boveda?q=%22%20(").await;
    assert_eq!((s, v["notas"].as_array().unwrap().len()), (StatusCode::OK, 0));
    let (s, _) = get(&f, "/api/boveda?region=inventada").await;
    assert_ne!(s, StatusCode::OK);

    let (s, v) = get(&f, &format!("/api/boveda/nota?ruta={ruta}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["cabecera"]["autor"], "modelo-local");
    assert!(v["contenido"].as_str().unwrap().contains("chorizo"));
    // Fuera de la bóveda, no.
    for mala in ["../etc/passwd", "/etc/passwd", "modelo-local/../../x.md"] {
        let (s, _) = get(&f, &format!("/api/boveda/nota?ruta={mala}")).await;
        assert_ne!(s, StatusCode::OK, "{mala}");
    }

    // Promoción del usuario desde la web: copia y deja el evento.
    let (s, v) = post(&f, "/api/boveda/promover", json!({ "ruta": ruta })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v) = get(&f, "/api/boveda?region=compartida").await;
    assert_eq!(v["notas"].as_array().unwrap().len(), 1);
    let (_, d) = get(&f, "/api/diario?limite=50").await;
    let acciones: Vec<_> = d.as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap().to_string()).collect();
    assert!(acciones.contains(&"boveda_promocion".to_string()) && acciones.contains(&"accion_ejecutada".to_string()));
    // Ya está: no se promueve dos veces; y lo que no es de un agente, tampoco.
    let (s, _) = post(&f, "/api/boveda/promover", json!({ "ruta": ruta })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/boveda/promover", json!({ "ruta": "compartida/receta-de-lentejas.md" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn aceptar_en_la_web_la_propuesta_de_promocion_de_un_agente_la_ejecuta() {
    let f = montar();
    let ruta = nota(&f, "claude-code", "Hallazgo", "Texto del hallazgo.");
    {
        let store = f.state.store.lock().await;
        let c = ea_acciones::SoloConsulta {
            rutinas_dir: f.state.config.rutinas_dir.clone(),
            boveda: f.state.boveda.clone(),
        };
        let args = json!({ "ruta": ruta });
        ea_acciones::proponer(&store, &c, &Default::default(), "claude-code", None, "ea_promover_nota", &args)
            .unwrap();
    }
    assert!(!f.state.boveda.raiz().join("compartida").exists());
    let (_, ps) = get(&f, "/api/propuestas?estado=pendiente").await;
    let id = ps[0]["id"].as_i64().unwrap();
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["ejecutada"]["estado"], "ejecutada", "{p}");
    assert!(f.state.boveda.raiz().join("compartida/hallazgo.md").exists());
}

/// Una acción lenta (un reinicio espera a `systemctl` hasta 30 s) no deja la
/// página sin datos: las lecturas van por otra conexión.
#[tokio::test]
async fn una_accion_en_curso_no_bloquea_las_lecturas() {
    let f = montar();
    let _en_curso = f.state.acciones.lock().await;

    for ruta in ["/api/estado", "/api/trabajos", "/api/diario", "/api/propuestas"] {
        let (s, _) = tokio::time::timeout(std::time::Duration::from_secs(5), get(&f, ruta))
            .await
            .unwrap_or_else(|_| panic!("{ruta} se quedó esperando a la acción en curso"));
        assert_eq!(s, StatusCode::OK, "{ruta}");
    }
}

#[tokio::test]
async fn la_portada_reune_las_propuestas_pendientes() {
    let f = montar();

    let (s, i) = get(&f, "/api/inicio").await;
    assert_eq!(s, StatusCode::OK);
    assert!(i["propuestas"].as_array().unwrap().is_empty());

    f.state.store.lock().await.crear_propuesta(texto("Revisar incidencias")).unwrap();

    let (_, i) = get(&f, "/api/inicio").await;
    assert_eq!(i["propuestas"][0]["descripcion"], "Revisar incidencias");
    assert!(i.get("activas").is_none());
}

#[tokio::test]
async fn la_consola_lista_simula_y_ejecuta_acciones() {
    let f = montar();
    let _g = aislar_bus(&f).await;

    let (s, cat) = get(&f, "/api/acciones").await;
    assert_eq!(s, StatusCode::OK);
    let de = |n: &str| cat.as_array().unwrap().iter().find(|h| h["nombre"] == n).unwrap().clone();
    assert_eq!(de("ea_status")["supervisada"], false);
    assert_eq!(de("ea_eliminar_rutina")["confirmable"], false);
    let encolar = de("ea_encolar_rutina");
    assert_eq!(encolar["confirmable"], true);
    assert_eq!(encolar["campos"][0]["opciones"], json!(["revision-diaria"]));

    let cuerpo = json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (s, sim) = post(&f, "/api/acciones/simular", cuerpo.clone()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(sim["texto"].as_str().unwrap().contains("revision-diaria"));
    assert!(!f._dir.path().join("estado/trabajos").exists() || std::fs::read_dir(f._dir.path().join("estado/trabajos")).unwrap().next().is_none());

    let ejecutar = json!({
        "herramienta": "ea_encolar_rutina",
        "argumentos": sim["argumentos"],
        "precondicion": sim["precondicion"],
    });
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar.clone()).await;
    assert_eq!(s, StatusCode::OK);
    let trabajo = r["datos"]["trabajo"].as_str().unwrap();
    assert!(f._dir.path().join("estado/trabajos").join(trabajo).join("orden.json").exists());

    // El mundo ya no es el de la simulación: ni simular ni ejecutar de nuevo.
    let (s, _) = post(&f, "/api/acciones/ejecutar", ejecutar).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/acciones/simular", cuerpo).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn la_consola_valida_y_no_ejecuta_irreversibles() {
    let f = montar();

    let (s, _) = post(&f, "/api/acciones/simular", json!({ "herramienta": "ea_status" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "solo las supervisadas");
    let (s, _) = post(
        &f,
        "/api/acciones/simular",
        json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": 1 } }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(
        &f,
        "/api/acciones/simular",
        json!({ "herramienta": "ea_reiniciar_servicio", "argumentos": { "servicio": "ea-mcp" } }),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);

    let borrar = json!({ "herramienta": "ea_eliminar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (s, sim) = post(&f, "/api/acciones/simular", borrar).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(sim["confirmable"], false);
    let (s, _) = post(
        &f,
        "/api/acciones/ejecutar",
        json!({
            "herramienta": "ea_eliminar_rutina",
            "argumentos": sim["argumentos"],
            "precondicion": sim["precondicion"],
        }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(f.state.config.rutinas_dir.join("revision-diaria.toml").exists());
}

#[tokio::test]
async fn las_sesiones_de_claude_code_solo_cuentan_si_el_proceso_vive() {
    let f = montar();
    let dir = f._dir.path().join("sesiones");
    let (s, v) = get(&f, "/api/sesiones").await;
    assert_eq!((s, v["sesiones"].as_array().unwrap().len()), (StatusCode::OK, 0), "sin directorio");

    std::fs::create_dir_all(&dir).unwrap();
    let propio = std::process::id();
    let ses = |pid: u32, nombre: &str| {
        json!({"pid": pid, "sessionId": "x", "cwd": "/tmp", "name": nombre, "status": "busy",
               "startedAt": 1000, "updatedAt": 2000})
        .to_string()
    };
    std::fs::write(dir.join(format!("{propio}.json")), ses(propio, "viva")).unwrap();
    std::fs::write(dir.join("999999999.json"), ses(999_999_999, "muerta")).unwrap();
    std::fs::write(dir.join("roto.json"), "no es json").unwrap();

    let (_, v) = get(&f, "/api/sesiones").await;
    let lista = v["sesiones"].as_array().unwrap();
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0]["nombre"], "viva");
    assert_eq!(lista[0]["ocupada"], true);
    assert_eq!(lista[0]["pid"], propio);
}

#[tokio::test]
async fn cada_sesion_lleva_sus_subagentes_en_marcha_primero() {
    let f = montar();
    let raiz = f._dir.path();
    let dir = raiz.join("sesiones");
    std::fs::create_dir_all(&dir).unwrap();
    let propio = std::process::id();
    let ses = |pid: u32, id: &str| {
        json!({"pid": pid, "sessionId": id, "cwd": "/tmp", "name": id, "status": "idle",
               "startedAt": 1000, "updatedAt": 2000})
        .to_string()
    };
    std::fs::write(dir.join(format!("{propio}.json")), ses(propio, "viva")).unwrap();
    std::fs::write(dir.join("999999999.json"), ses(999_999_999, "muerta")).unwrap();

    // La transcripción del padre y el meta.json de un subagente.
    let transcript = raiz.join("proyecto/viva.jsonl");
    std::fs::create_dir_all(raiz.join("proyecto/viva/subagents")).unwrap();
    std::fs::write(
        raiz.join("proyecto/viva/subagents/agent-a1.meta.json"),
        json!({"agentType": "Explore", "description": "Buscar el tablero", "model": "haiku"}).to_string(),
    )
    .unwrap();
    let marca = |sesion: &str, id: &str, desde: u64, hasta: Option<u64>| {
        let d = raiz.join("subagentes").join(sesion);
        std::fs::create_dir_all(&d).unwrap();
        let mut m = json!({"agente": id, "tipo": "Explore", "transcript": transcript, "desde": desde});
        if let Some(h) = hasta {
            m["hasta"] = json!(h);
        }
        std::fs::write(d.join(format!("{id}.json")), m.to_string()).unwrap();
    };
    marca("viva", "a0", 100, Some(200));
    marca("viva", "a2", 150, Some(300));
    marca("viva", "a1", 400, None);
    marca("muerta", "a9", 100, None);

    let (_, v) = get(&f, "/api/sesiones").await;
    let lista = v["sesiones"].as_array().unwrap();
    assert_eq!(lista.len(), 1);
    let subs = lista[0]["subagentes"].as_array().unwrap();
    let ids: Vec<_> = subs.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a1", "a2", "a0"], "en marcha primero, luego los terminados más recientes");
    assert_eq!(subs[0]["descripcion"], "Buscar el tablero");
    assert_eq!(subs[0]["modelo"], "haiku");
    assert!(subs[0]["hasta"].is_null());
    assert_eq!(subs[1]["descripcion"], "Explore", "sin meta.json se usa el tipo");
    assert_eq!(subs[1]["hasta"], 300);
}

// ----------------------------------------------------------------- memoria

const MEMORIA_EJEMPLO: &str = "---\nname: refactor\ndescription: \"Qué se refactorizó\"\nmetadata:\n  node_type: memory\n  type: project\n---\n\nCuerpo con [[otra]].\n";

/// Un proyecto de Claude Code con dos memorias, el índice y una transcripción
/// que cuenta su cwd. Además, uno sin `memory/` y otro con ella vacía.
fn montar_memoria(f: &Fixture) -> std::path::PathBuf {
    let raiz = f._dir.path().join("proyectos");
    let mem = raiz.join("-home-u-proy/memory");
    std::fs::create_dir_all(&mem).unwrap();
    std::fs::write(mem.join("refactor.md"), MEMORIA_EJEMPLO).unwrap();
    std::fs::write(mem.join("MEMORY.md"), "- [Refactor](refactor.md) — gancho\n").unwrap();
    std::fs::write(mem.join("notas.txt"), "no es markdown").unwrap();
    std::fs::write(
        raiz.join("-home-u-proy/sesion.jsonl"),
        "{\"type\":\"x\"}\nbasura\n{\"cwd\":\"/home/u/proy\",\"type\":\"user\"}\n",
    )
    .unwrap();
    std::fs::create_dir_all(raiz.join("-sin-memoria")).unwrap();
    std::fs::create_dir_all(raiz.join("-vacia/memory")).unwrap();
    mem
}

#[tokio::test]
async fn la_memoria_lista_solo_proyectos_con_memorias() {
    let f = montar();
    let (_, v) = get(&f, "/api/memoria").await;
    assert_eq!(v["proyectos"].as_array().unwrap().len(), 0, "sin directorio");
    montar_memoria(&f);
    let (s, v) = get(&f, "/api/memoria").await;
    assert_eq!(s, StatusCode::OK);
    let ps = v["proyectos"].as_array().unwrap();
    assert_eq!(ps.len(), 1, "{v}");
    assert_eq!((ps[0]["id"].as_str(), ps[0]["cwd"].as_str()), (Some("-home-u-proy"), Some("/home/u/proy")));
    let fs = ps[0]["ficheros"].as_array().unwrap();
    let nombres: Vec<_> = fs.iter().map(|x| x["nombre"].as_str().unwrap()).collect();
    assert_eq!(nombres, ["MEMORY.md", "refactor.md"], "el índice primero y solo .md");
    assert_eq!(fs[1]["titulo"], "refactor");
    assert_eq!(fs[1]["descripcion"], "Qué se refactorizó");
    assert_eq!(fs[1]["tipo"], "project");
    assert!(fs[0]["titulo"].is_null());
}

#[tokio::test]
async fn la_memoria_se_lee_con_su_version() {
    let f = montar();
    montar_memoria(&f);
    let (s, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["contenido"], MEMORIA_EJEMPLO);
    assert_eq!(v["version"].as_str().unwrap().len(), 16);
}

#[tokio::test]
async fn guardar_la_memoria_cambia_el_disco_y_deja_rastro_en_el_diario() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let cuerpo = json!({ "contenido": "---\nname: nuevo\n---\ntexto\n", "version": v["version"] });
    let (s, nueva) = post(&f, "/api/memoria/-home-u-proy/refactor.md", cuerpo).await;
    assert_eq!(s, StatusCode::OK, "{nueva}");
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), "---\nname: nuevo\n---\ntexto\n");
    assert_ne!(nueva["version"], v["version"]);
    assert_eq!(nueva["contenido"], "---\nname: nuevo\n---\ntexto\n");
    let temporales = std::fs::read_dir(&mem).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).count();
    assert_eq!(temporales, 0, "sin temporales");
    let (_, d) = get(&f, "/api/diario").await;
    assert!(d.as_array().unwrap().iter().any(|e| e["accion"] == "memoria_editada" && e["actor"] == "usuario"), "{d}");
    // Con la versión nueva se puede volver a guardar.
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/refactor.md", json!({ "contenido": "x", "version": nueva["version"] })).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn guardar_con_una_version_vieja_da_conflicto_y_no_toca_el_disco() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    std::fs::write(mem.join("refactor.md"), "cambiada a mano").unwrap();
    let (s, e) = post(&f, "/api/memoria/-home-u-proy/refactor.md", json!({ "contenido": "mío", "version": v["version"] })).await;
    assert_eq!(s, StatusCode::CONFLICT, "{e}");
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), "cambiada a mano");
}

#[tokio::test]
async fn la_memoria_rechaza_nombres_maliciosos() {
    let f = montar();
    let mem = montar_memoria(&f);
    let fuera = f._dir.path().join("secreto.md");
    std::fs::write(&fuera, "fuera").unwrap();
    std::os::unix::fs::symlink(&fuera, mem.join("enlace.md")).unwrap();
    std::fs::write(mem.join(".oculto.md"), "oculto").unwrap();
    let rutas = [
        "/api/memoria/%2E%2E/refactor.md",
        "/api/memoria/-home-u-proy/%2E%2E",
        "/api/memoria/x%2F..%2Fy/refactor.md",
        "/api/memoria/-home-u-proy/..%2F..%2Fsecreto.md",
        "/api/memoria/-home-u-proy/notas.txt",
        "/api/memoria/-home-u-proy/.oculto.md",
        "/api/memoria/-home-u-proy/enlace.md",
    ];
    for r in rutas {
        let (s, _) = get(&f, r).await;
        assert!(matches!(s, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "GET {r}: {s}");
        let (s, _) = post(&f, r, json!({ "contenido": "pwned", "version": "0" })).await;
        assert!(matches!(s, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "POST {r}: {s}");
    }
    assert_eq!(std::fs::read_to_string(&fuera).unwrap(), "fuera");
    assert_eq!(std::fs::read_to_string(mem.join(".oculto.md")).unwrap(), "oculto");
    assert_eq!(std::fs::read_to_string(mem.join("notas.txt")).unwrap(), "no es markdown");
}

#[tokio::test]
async fn no_se_crean_memorias_desde_la_web() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/nueva.md", json!({ "contenido": "x", "version": "0" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(!mem.join("nueva.md").exists());
    let (s, _) = post(&f, "/api/memoria/-no-existe/refactor.md", json!({ "contenido": "x", "version": "0" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn guardar_la_memoria_exige_json_y_un_tamano_razonable() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (s, _) = enviar(&f, Method::POST, "/api/memoria/-home-u-proy/refactor.md", "localhost:8092", Some(json!({})), false).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let enorme = "a".repeat(256 * 1024 + 1);
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/refactor.md", json!({ "contenido": enorme, "version": v["version"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), MEMORIA_EJEMPLO);
}

#[tokio::test]
async fn la_memoria_se_lee_con_su_ruta_absoluta() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    assert_eq!(v["ruta"], mem.join("refactor.md").display().to_string());
}

#[tokio::test]
async fn eliminar_una_memoria_la_borra_y_guarda_su_contenido_en_el_diario() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let (s, e) = post(&f, "/api/memoria/-home-u-proy/refactor.md/eliminar", json!({ "version": v["version"] })).await;
    assert_eq!(s, StatusCode::OK, "{e}");
    assert!(!mem.join("refactor.md").exists());
    let (_, d) = get(&f, "/api/diario").await;
    let ev = d.as_array().unwrap().iter().find(|e| e["accion"] == "memoria_eliminada").expect("evento");
    assert_eq!(ev["actor"], "usuario");
    assert!(ev.to_string().contains("Cuerpo con"), "el contenido queda en el diario: {ev}");
}

#[tokio::test]
async fn eliminar_exige_la_version_y_no_toca_el_indice_ni_fuera() {
    let f = montar();
    let mem = montar_memoria(&f);
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    std::fs::write(mem.join("refactor.md"), "cambiada a mano").unwrap();
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/refactor.md/eliminar", json!({ "version": v["version"] })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(mem.join("refactor.md").exists());

    let (_, i) = get(&f, "/api/memoria/-home-u-proy/MEMORY.md").await;
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/MEMORY.md/eliminar", json!({ "version": i["version"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(mem.join("MEMORY.md").exists());

    let fuera = f._dir.path().join("secreto.md");
    std::fs::write(&fuera, "fuera").unwrap();
    std::os::unix::fs::symlink(&fuera, mem.join("enlace.md")).unwrap();
    for r in ["/api/memoria/-home-u-proy/enlace.md/eliminar", "/api/memoria/-home-u-proy/..%2F..%2Fsecreto.md/eliminar"] {
        let (s, _) = post(&f, r, json!({ "version": memoria_version("fuera") })).await;
        assert!(matches!(s, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "{r}: {s}");
    }
    assert!(fuera.exists() && mem.join("enlace.md").exists());

    let (s, _) = enviar(&f, Method::POST, "/api/memoria/-home-u-proy/refactor.md/eliminar", "localhost:8092", Some(json!({})), false).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

fn memoria_version(contenido: &str) -> String {
    ea_web::memoria::version(contenido.as_bytes())
}

#[tokio::test]
async fn renombrar_mueve_la_memoria_y_reescribe_los_enlaces() {
    let f = montar();
    let mem = montar_memoria(&f);
    std::fs::write(mem.join("otra.md"), "Ver [[refactor|el refactor]] y [[refactorizar]].\n").unwrap();
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let (s, r) = post(&f, "/api/memoria/-home-u-proy/refactor.md/renombrar", json!({ "nuevo": "rehecho.md", "version": v["version"] })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["memoria"]["nombre"], "rehecho.md");
    assert_eq!(r["memoria"]["contenido"], MEMORIA_EJEMPLO);
    assert!(!mem.join("refactor.md").exists());
    let mut act: Vec<_> = r["actualizadas"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    act.sort();
    assert_eq!(act, ["MEMORY.md", "otra.md"]);
    assert_eq!(std::fs::read_to_string(mem.join("MEMORY.md")).unwrap(), "- [Refactor](rehecho.md) — gancho\n");
    assert_eq!(std::fs::read_to_string(mem.join("otra.md")).unwrap(), "Ver [[rehecho|el refactor]] y [[refactorizar]].\n");
    let (_, d) = get(&f, "/api/diario").await;
    assert!(d.as_array().unwrap().iter().any(|e| e["accion"] == "memoria_renombrada"), "{d}");
}

#[tokio::test]
async fn renombrar_no_pisa_ni_acepta_nombres_malos() {
    let f = montar();
    let mem = montar_memoria(&f);
    std::fs::write(mem.join("otra.md"), "otra").unwrap();
    let (_, v) = get(&f, "/api/memoria/-home-u-proy/refactor.md").await;
    let ruta = "/api/memoria/-home-u-proy/refactor.md/renombrar";
    let (s, _) = post(&f, ruta, json!({ "nuevo": "otra.md", "version": v["version"] })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(std::fs::read_to_string(mem.join("otra.md")).unwrap(), "otra");
    assert_eq!(std::fs::read_to_string(mem.join("refactor.md")).unwrap(), MEMORIA_EJEMPLO);
    for nuevo in ["../x.md", ".oculto.md", "x.txt", "MEMORY.md", "a/b.md", "refactor.md"] {
        let (s, _) = post(&f, ruta, json!({ "nuevo": nuevo, "version": v["version"] })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{nuevo}");
    }
    let (s, _) = post(&f, ruta, json!({ "nuevo": "nueva.md", "version": "0" })).await;
    assert_eq!(s, StatusCode::CONFLICT, "versión vieja");
    let (_, i) = get(&f, "/api/memoria/-home-u-proy/MEMORY.md").await;
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/MEMORY.md/renombrar", json!({ "nuevo": "indice.md", "version": i["version"] })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(mem.join("refactor.md").exists() && mem.join("MEMORY.md").exists() && !mem.join("nueva.md").exists());
}

#[tokio::test]
async fn mostrar_una_memoria_que_no_existe_da_404() {
    let f = montar();
    montar_memoria(&f);
    let (s, _) = post(&f, "/api/memoria/-home-u-proy/no-esta.md/mostrar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ------------------------------------------------------- editor de la bóveda

fn acciones_del_diario(d: &Value) -> Vec<String> {
    d.as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn el_arbol_de_la_boveda_lleva_regiones_carpetas_y_notas() {
    let f = montar();
    nota(&f, "claude-code", "Una", "uno");
    std::fs::create_dir_all(f.state.boveda.raiz().join("modelo-local/vacia")).unwrap();
    let (s, v) = get(&f, "/api/boveda/arbol").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["regiones"], json!(["modelo-local", "claude-code", "compartida"]));
    assert_eq!(v["notas"][0]["ruta"], "claude-code/una.md");
    let carpetas: Vec<_> = v["carpetas"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(carpetas.contains(&"modelo-local/vacia") && carpetas.contains(&"claude-code"));
    assert_eq!(v["en_papelera"], 0);
}

#[tokio::test]
async fn la_nota_se_lee_entera_y_se_guarda_con_su_version() {
    let f = montar();
    let ruta = nota(&f, "modelo-local", "Receta", "Lentejas.");
    let (s, v) = get(&f, &format!("/api/boveda/fichero?ruta={ruta}")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let contenido = v["contenido"].as_str().unwrap().to_string();
    assert!(contenido.starts_with("---\n") && contenido.contains("Lentejas."));
    assert!(v["absoluta"].as_str().unwrap().ends_with("/boveda/modelo-local/receta.md"));
    let version = v["version"].as_str().unwrap().to_string();

    let nuevo = contenido.replace("Lentejas.", "Garbanzos.");
    let (s, g) = post(&f, "/api/boveda/guardar", json!({ "ruta": ruta, "contenido": nuevo, "version": version })).await;
    assert_eq!(s, StatusCode::OK, "{g}");
    assert!(std::fs::read_to_string(f.state.boveda.raiz().join(&ruta)).unwrap().contains("Garbanzos."));
    let (_, b) = get(&f, "/api/boveda?q=garbanzos").await;
    assert_eq!(b["notas"].as_array().unwrap().len(), 1);
    let (_, d) = get(&f, "/api/diario?limite=50").await;
    assert!(acciones_del_diario(&d).contains(&"boveda_edicion".to_string()));

    // Con la versión vieja: 409 y el disco no cambia.
    let (s, _) = post(&f, "/api/boveda/guardar", json!({ "ruta": ruta, "contenido": "otra", "version": version })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(std::fs::read_to_string(f.state.boveda.raiz().join(&ruta)).unwrap().contains("Garbanzos."));
    // Cambiar la firma: 400.
    let v2 = g["version"].as_str().unwrap();
    let firma = g["contenido"].as_str().unwrap().replace("autor: modelo-local", "autor: usuario");
    let (s, _) = post(&f, "/api/boveda/guardar", json!({ "ruta": ruta, "contenido": firma, "version": v2 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&f, "/api/boveda/fichero?ruta=compartida/no-esta.md").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn crear_solo_en_la_compartida() {
    let f = montar();
    let (s, v) = post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/ideas/nueva" })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["ruta"], "compartida/ideas/nueva.md");
    assert_eq!(v["cabecera"]["autor"], "usuario");
    let (s, _) = post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/ideas/nueva.md" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/boveda/crear", json!({ "ruta": "claude-code/mia.md" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(!f.state.boveda.raiz().join("claude-code/mia.md").exists());
    let (s, _) = post(&f, "/api/boveda/carpeta", json!({ "ruta": "claude-code/carpeta" })).await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(f.state.boveda.raiz().join("claude-code/carpeta").is_dir());
}

#[tokio::test]
async fn renombrar_en_la_boveda_reescribe_los_enlaces() {
    let f = montar();
    let (_, x) = post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/x", "contenido": "soy x" })).await;
    post(&f, "/api/boveda/crear", json!({ "ruta": "compartida/otra", "contenido": "[[x]] y [x](x.md)" })).await;
    let (s, r) = post(
        &f,
        "/api/boveda/renombrar",
        json!({ "ruta": "compartida/x.md", "nuevo": "compartida/sub/y", "version": x["version"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["nota"]["ruta"], "compartida/sub/y.md");
    assert_eq!(r["actualizadas"], json!(["compartida/otra.md"]));
    let otra = std::fs::read_to_string(f.state.boveda.raiz().join("compartida/otra.md")).unwrap();
    assert!(otra.contains("[[y]] y [x](sub/y.md)"), "{otra}");
    // No pisa otra nota ni cambia de región.
    let v = r["nota"]["version"].clone();
    let (s, _) = post(&f, "/api/boveda/renombrar", json!({ "ruta": "compartida/sub/y.md", "nuevo": "compartida/otra", "version": v })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = post(&f, "/api/boveda/renombrar", json!({ "ruta": "compartida/sub/y.md", "nuevo": "claude-code/y", "version": v })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn eliminar_manda_a_la_papelera_y_se_restaura() {
    let f = montar();
    let ruta = nota(&f, "claude-code", "Borrable", "contenido");
    let (_, n) = get(&f, &format!("/api/boveda/fichero?ruta={ruta}")).await;
    let (s, p) = post(&f, "/api/boveda/eliminar", json!({ "ruta": ruta, "version": n["version"] })).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert!(!f.state.boveda.raiz().join(&ruta).exists());
    let (_, a) = get(&f, "/api/boveda/arbol").await;
    assert_eq!((a["notas"].as_array().unwrap().len(), a["en_papelera"].as_u64()), (0, Some(1)));
    let (_, l) = get(&f, "/api/boveda/papelera").await;
    assert_eq!(l[0]["ruta"], ruta.as_str());
    let (s, r) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": p["id"] })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(r["contenido"].as_str().unwrap().contains("contenido"));
    let (_, d) = get(&f, "/api/diario?limite=50").await;
    let acc = acciones_del_diario(&d);
    assert!(acc.contains(&"boveda_papelera".to_string()) && acc.contains(&"boveda_restauracion".to_string()));
    let (s, _) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": p["id"] })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_editor_de_la_boveda_rechaza_rutas_maliciosas_y_exige_json() {
    let f = montar();
    nota(&f, "claude-code", "Algo", "x");
    for mala in ["../etc/passwd", "/etc/passwd", "claude-code/../../x.md", ".papelera/x.md", "claude-code/.x.md"] {
        let (s, _) = get(&f, &format!("/api/boveda/fichero?ruta={mala}")).await;
        assert_ne!(s, StatusCode::OK, "{mala}");
        let (s, _) = post(&f, "/api/boveda/guardar", json!({ "ruta": mala, "contenido": "x", "version": "0" })).await;
        assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::FORBIDDEN, "{mala}: {s}");
        let (s, _) = post(&f, "/api/boveda/crear", json!({ "ruta": mala })).await;
        assert!(s.is_client_error(), "{mala}: {s}");
    }
    let (s, _) = post(&f, "/api/boveda/papelera/restaurar", json!({ "id": "../../etc" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = enviar(&f, Method::POST, "/api/boveda/crear", "localhost:8092", Some(json!({ "ruta": "compartida/x" })), false).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!f.state.boveda.raiz().join("compartida/x.md").exists());
    let (s, _) = post(&f, "/api/boveda/mostrar", json!({ "ruta": "compartida/no-esta.md" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ----------------------------------------------------------------- ajustes

fn clave_de<'a>(v: &'a Value, clave: &str) -> &'a Value {
    v["secciones"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["claves"].as_array().unwrap())
        .find(|c| c["clave"] == clave)
        .unwrap_or_else(|| panic!("falta {clave}"))
}

#[tokio::test]
async fn los_ajustes_se_ven_con_origen_y_edicion() {
    let f = montar();
    let (s, v) = get(&f, "/api/ajustes").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["existe"], false);
    assert_eq!(v["version"], "");
    assert!(v["fichero"].as_str().unwrap().starts_with(f._dir.path().to_str().unwrap()));
    assert_eq!(v["cliente"], json!({ "sondeo_ms": 3000, "diario_limite": 300 }));
    let ids: Vec<&str> = v["secciones"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["modelo", "agente", "orquestador", "red", "rutas", "web"]);
    let turnos = clave_de(&v, "agente.techo_topes.turnos");
    assert_eq!((turnos["valor"].clone(), turnos["origen"]["tipo"].clone()), (json!(100), json!("defecto")));
    assert_eq!((turnos["edicion"].clone(), turnos["endurecible"].clone()), (json!("web"), json!(true)));
    assert_eq!(clave_de(&v, "agente.sandbox")["edicion"], "terminal");
    assert_eq!(clave_de(&v, "red.puerto_web")["reinicio"], json!(["ea-web"]));
}

#[tokio::test]
async fn cambiar_un_ajuste_escribe_el_fichero_y_lo_apunta() {
    let f = montar();
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "web.sondeo_ms": 5000 } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["aplicados"][0], json!({ "clave": "web.sondeo_ms", "antes": 3000, "despues": 5000, "aplica": "caliente", "reinicio": [] }));
    assert_eq!(v["ajustes"]["cliente"]["sondeo_ms"], 5000);
    let fichero = f._dir.path().join("config/ajustes.toml");
    assert!(std::fs::read_to_string(&fichero).unwrap().contains("sondeo_ms = 5000"));
    let (_, d) = get(&f, "/api/diario").await;
    let ev = d.as_array().unwrap().iter().find(|e| e["accion"] == "ajustes_cambiados").expect("evento");
    assert_eq!(ev["actor"], "usuario");
    // versión vieja: 409 y el fichero no cambia
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "web.sondeo_ms": 6000 } })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    // null restablece
    let version = v["ajustes"]["version"].clone();
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "web.sondeo_ms": null } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["ajustes"]["cliente"]["sondeo_ms"], 3000);
}

#[tokio::test]
async fn la_web_no_afloja_ni_toca_lo_de_terminal() {
    let f = montar();
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "agente.sandbox": false } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "agente.skills.pueden_elegir": [] } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": "", "cambios": { "agente.techo_topes.turnos": 20 } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let version = v["ajustes"]["version"].clone();
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "agente.techo_topes.turnos": 50 } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "modelo.base_url": "http://192.168.1.2:8080/v1" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "no.existe": 1 } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let cuerpo = json!({ "version": version, "cambios": { "web.sondeo_ms": 5000 } });
    let (s, _) = enviar(&f, Method::POST, "/api/ajustes", "localhost:8092", Some(cuerpo.clone()), false).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (s, _) = enviar(&f, Method::POST, "/api/ajustes", "evil.example", Some(cuerpo), true).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, v) = get(&f, "/api/ajustes").await;
    assert_eq!(clave_de(&v, "agente.techo_topes.turnos")["valor"], 20);
    assert_eq!(clave_de(&v, "agente.sandbox")["valor"], true);
}
