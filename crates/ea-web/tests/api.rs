use std::sync::Arc;

/// Un directorio absoluto que existe en cada sistema (`/tmp` no es absoluta en Windows).
#[cfg(windows)]
macro_rules! tmp {
    () => {
        "C:/Windows"
    };
}
#[cfg(not(windows))]
macro_rules! tmp {
    () => {
        "/tmp"
    };
}

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
    /// Las terminales que la sección Biblioteca habría abierto.
    terminales: Arc<std::sync::Mutex<Vec<Vec<String>>>>,
}

fn texto(descripcion: &str) -> ea_store::NuevaPropuesta {
    ea_store::NuevaPropuesta { proponente: "claude-code".into(), descripcion: descripcion.into(), accion: None }
}

const RUTINA_BUS: &str = concat!("[rutina]\nslug = \"revision-diaria\"\nversion = 1\nworktree = \"", tmp!(), "\"\ncriterio_exito = \"test -f {salida}\"\n[entradas]\nsalida = \"informe.md\"\n[[pasos]]\ntexto = \"revisa y escribe {salida}\"\n");

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
    let terminal = ea_web::servicio::terminal::Terminal::simulada(dir.path().join("bin/ea"));
    let terminales = terminal.1.clone();
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
        temporizadores: temporizadores_falsos(dir.path()),
        terminal: terminal.0,
        biblioteca_raices: Some(raices_biblioteca(dir.path())),
    };
    let state = Arc::new(AppState::new(store, config).expect("estado"));
    Fixture { app: api::router(state.clone()), state, _dir: dir, terminales }
}

// ---------------------------------------------------------------- biblioteca

/// Lo que no puede salir nunca en una respuesta: está en los ficheros del HOME de prueba.
const SECRETOS: [&str; 7] = [
    "SECRETO_ENV_CLAUDE",
    "SECRETO_ARG_TOKEN",
    "SECRETO_HEADER_CLAUDE",
    "SECRETO_RUTA_URL",
    "SECRETO_CONSULTA_URL",
    "SECRETO_ENV_CODEX",
    "SECRETO_HEADER_OPENCODE",
];

fn escribir(ruta: impl AsRef<std::path::Path>, texto: &str) {
    let ruta = ruta.as_ref();
    std::fs::create_dir_all(ruta.parent().unwrap()).unwrap();
    std::fs::write(ruta, texto).unwrap();
}

const SKILL: &str = "claude-code/skill/usuario/propia";
const MCP_CLAUDE: &str = "claude-code/mcp/usuario/remoto-claude";
const MCP_CODEX: &str = "codex/mcp/usuario/docs";
const MCP_CODEX_APAGADO: &str = "codex/mcp/usuario/apagado";

/// Un HOME de prueba con un elemento de varios tipos y agentes, y secretos en
/// sus MCP; el proyecto, el estado y el runtime, también temporales.
fn raices_biblioteca(dir: &std::path::Path) -> ea_biblioteca::Raices {
    let home = dir.join("home");
    let proyecto = dir.join("proyecto");
    std::fs::create_dir_all(&proyecto).unwrap();
    escribir(home.join(".claude/skills/propia/SKILL.md"), "---\ndescription: Hace cosas\n---\n");
    escribir(
        home.join(".claude.json"),
        r#"{"mcpServers":{
            "stdio-claude":{"type":"stdio","command":"npx","args":["-y","@scope/server","--token","SECRETO_ARG_TOKEN"],"env":{"API_KEY":"SECRETO_ENV_CLAUDE"}},
            "remoto-claude":{"type":"http","url":"https://mcp.ejemplo.com:8443/SECRETO_RUTA_URL/mcp?k=SECRETO_CONSULTA_URL","headers":{"Authorization":"Bearer SECRETO_HEADER_CLAUDE"}}},
          "skillUsage":{"propia":{"usageCount":4,"lastUsedAt":1791408135818}}}"#,
    );
    escribir(
        home.join(".codex/config.toml"),
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--api-key=SECRETO_ENV_CODEX\"]\n[mcp_servers.docs.env]\nTOKEN = \"SECRETO_ENV_CODEX\"\n[mcp_servers.apagado]\ncommand = \"otro\"\nenabled = false\n",
    );
    escribir(
        home.join(".config/opencode/opencode.json"),
        r#"{"mcp":{"servers":{"remoto-oc":{"type":"remote","url":"https://oc.ejemplo.com/SECRETO_RUTA_URL","headers":{"Authorization":"SECRETO_HEADER_OPENCODE"}}}}}"#,
    );
    let mut r = ea_biblioteca::Raices::nuevo(home, Some(proyecto), dir.join("estado-biblioteca"), Some(dir.join("runtime")));
    r.repo = None;
    r
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

// ------------------------------------------------- D4: anotar el diario

/// Todas las filas del diario, latidos incluidos, tal como las guarda la base.
async fn filas_del_diario(f: &Fixture) -> Vec<ea_store::Event> {
    let filtro = ea_store::EventFilter::default();
    f.state.store.lock().await.list_events(&filtro).unwrap()
}

#[tokio::test]
async fn anotar_el_diario_anade_un_evento_y_no_modifica_ninguno() {
    let f = montar();
    let original = f
        .state
        .store
        .lock()
        .await
        .append_event(NewEvent {
            actor: "orquestador".into(),
            action: "bus_entregar".into(),
            trabajo: Some("m-1".into()),
            outcome: Some(EventOutcome::Error),
            duration_ms: Some(42),
            detail: Some("{\"motivo\":\"equivocado\"}".into()),
        })
        .unwrap();
    let antes = filas_del_diario(&f).await;

    // Constancia suelta, con trabajo.
    let (s, n1) = post(&f, "/api/diario/anotar", json!({"texto": "he reiniciado llama-server a mano", "trabajo": "m-1"})).await;
    assert_eq!(s, StatusCode::CREATED, "{n1}");
    assert_eq!(n1["actor"], "usuario");
    assert_eq!(n1["accion"], "usuario_nota");
    assert_eq!(n1["trabajo"], "m-1");
    let d1: Value = serde_json::from_str(n1["detalle"].as_str().unwrap()).unwrap();
    assert_eq!(d1, json!({"texto": "he reiniciado llama-server a mano", "trabajo": "m-1"}));

    // Corrección del evento original.
    let (s, n2) = post(&f, "/api/diario/anotar", json!({"texto": "no falló: lo cancelé yo", "corrige": original.id})).await;
    assert_eq!(s, StatusCode::CREATED, "{n2}");
    let d2: Value = serde_json::from_str(n2["detalle"].as_str().unwrap()).unwrap();
    assert_eq!(d2["corrige"], original.id);

    // Dos filas más, y las de antes, idénticas.
    let despues = filas_del_diario(&f).await;
    assert_eq!(despues.len(), antes.len() + 2);
    for e in &antes {
        let ahora = despues.iter().find(|x| x.id == e.id).expect("sigue estando");
        assert_eq!(serde_json::to_value(ahora).unwrap(), serde_json::to_value(e).unwrap());
    }

    // El corregido lo dice en el listado; la nota está en la familia `usuario_`.
    let (_, d) = get(&f, "/api/diario").await;
    let fila = d.as_array().unwrap().iter().find(|e| e["id"] == original.id).unwrap();
    assert_eq!(fila["corregido_por"], json!([n2["id"]]));
    assert_eq!(fila["detalle"], "{\"motivo\":\"equivocado\"}");
    let (_, d) = get(&f, "/api/diario?accion=usuario_").await;
    assert_eq!(acciones(&d), ["usuario_nota", "usuario_nota"]);
}

#[tokio::test]
async fn anotar_el_diario_rechaza_lo_invalido_sin_escribir() {
    let f = montar();
    let antes = filas_del_diario(&f).await.len();
    for cuerpo in [
        json!({"texto": ""}),
        json!({"texto": "   \n "}),
        json!({"texto": "x".repeat(10_001)}),
        json!({"texto": "x", "corrige": 999}),
        json!({"texto": "x", "trabajo": "../etc"}),
        json!({"texto": "x", "otro": 1}),
        json!({"corrige": 1}),
        json!({"texto": 3}),
    ] {
        let (s, r) = post(&f, "/api/diario/anotar", cuerpo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}: {r}");
        assert!(r["error"].is_string(), "mensaje: {r}");
    }
    assert_eq!(filas_del_diario(&f).await.len(), antes);
    // Justo en el tope, sí.
    let (s, _) = post(&f, "/api/diario/anotar", json!({"texto": "ñ".repeat(10_000)})).await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn anotar_el_diario_sin_clave_da_403() {
    let f = montar();
    for clave in [None, Some("mala")] {
        let (s, _) = enviar_con_clave(
            &f,
            Method::POST,
            "/api/diario/anotar",
            "localhost:8092",
            Some(json!({"texto": "suplantación"})),
            true,
            clave,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{clave:?}");
    }
    assert!(filas_del_diario(&f).await.is_empty());
}

#[tokio::test]
async fn el_diario_pagina_por_cursor_sin_solapes() {
    let f = montar();
    let anotar = |actor: &str, accion: &str, ok: bool| NewEvent {
        actor: actor.into(),
        action: accion.into(),
        trabajo: None,
        outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
        duration_ms: None,
        detail: None,
    };
    {
        let store = f.state.store.lock().await;
        // 11 errores del bus entre otros eventos que el filtro descarta.
        for i in 0..11 {
            store.append_event(anotar("orquestador", "bus_entregar", false)).unwrap();
            store.append_event(anotar("orquestador", "bus_entregar", true)).unwrap();
            if i % 2 == 0 {
                store.append_event(anotar("usuario", "propuesta_decidida", false)).unwrap();
            }
        }
    }

    let filtro = "accion=bus_&resultado=error&limite=4";
    let mut ids: Vec<i64> = Vec::new();
    let mut antes: Option<i64> = None;
    let mut paginas = 0;
    loop {
        let ruta = match antes {
            Some(id) => format!("/api/diario?{filtro}&antes_de={id}"),
            None => format!("/api/diario?{filtro}"),
        };
        let (s, d) = get(&f, &ruta).await;
        assert_eq!(s, StatusCode::OK, "{ruta}");
        let pagina = d.as_array().unwrap();
        if pagina.is_empty() {
            break;
        }
        paginas += 1;
        assert!(pagina.len() <= 4);
        for e in pagina {
            assert_eq!(e["accion"], "bus_entregar");
            assert_eq!(e["resultado"], "error");
        }
        if paginas == 1 {
            // Un evento nuevo entre página y página no desplaza el cursor.
            let store = f.state.store.lock().await;
            store.append_event(anotar("orquestador", "bus_entregar", false)).unwrap();
        }
        ids.extend(pagina.iter().map(|e| e["id"].as_i64().unwrap()));
        antes = ids.last().copied();
    }
    assert_eq!(paginas, 3, "4 + 4 + 3");
    assert_eq!(ids.len(), 11, "todos, sin huecos: {ids:?}");
    let mut esperado = ids.clone();
    esperado.sort_unstable_by(|a, b| b.cmp(a));
    esperado.dedup();
    assert_eq!(ids, esperado, "de más reciente a más antiguo y sin repetidos");

    // La primera página (lo que refresca el sondeo) ya trae el nuevo.
    let (_, d) = get(&f, &format!("/api/diario?{filtro}")).await;
    assert!(d[0]["id"].as_i64().unwrap() > ids[0]);

    for q in ["antes_de=abc", "antes_de=-3", "antes_de=0", "antes_de=1.5"] {
        let (s, _) = get(&f, &format!("/api/diario?{q}")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{q}");
    }
    // Vacío, como lo manda la barra sin cursor: no pagina.
    let (s, _) = get(&f, "/api/diario?antes_de=").await;
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

#[tokio::test]
async fn la_ficha_de_una_rutina_lista_sus_ejecuciones_mas_recientes_primero() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let bus = ea_bus::Bus::abrir().unwrap();

    // Sin trabajos: la fila no tiene última y la ficha no tiene ejecuciones.
    let (_, l) = get(&f, "/api/rutinas").await;
    assert!(l[0]["ultima"].is_null(), "{l}");
    let (_, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(r["ejecuciones"], json!([]));

    // Doce ejecuciones (v1 las seis primeras, v2 el resto), intercaladas con
    // una misión y con otra rutina. Los ids fijan el orden (empiezan por la
    // fecha); se crean desordenados para que no cuente el orden de creación.
    let de_rutina = |n: u32, slug: &str, version: u32| {
        let mut o = orden(ea_bus::Modo::Rutina, slug, tmp!());
        o.id = format!("20260101T0000{n:02}-0000aaaa");
        o.rutina = Some(ea_bus::RutinaRef { slug: slug.into(), version });
        o
    };
    for n in (1..=12).rev() {
        bus.crear_trabajo(de_rutina(n, "revision-diaria", if n <= 6 { 1 } else { 2 })).unwrap();
    }
    let mut otra = de_rutina(13, "otra-rutina", 1);
    otra.id = "20260101T000013-0000bbbb".into();
    bus.crear_trabajo(otra).unwrap();
    let mut mision = orden(ea_bus::Modo::Mision, "Una misión", tmp!());
    mision.id = "20260101T000014-0000cccc".into();
    bus.crear_trabajo(mision).unwrap();
    // La más reciente entregó informe; la anterior se canceló.
    let ultimo = "20260101T000012-0000aaaa";
    bus.aplicar(ultimo, ea_bus::Evento::Tomar).unwrap();
    let informe = ea_bus::Informe { duracion_s: 42, ..ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Bloqueado) };
    bus.aplicar(ultimo, ea_bus::Evento::Entregar(informe)).unwrap();
    bus.aplicar("20260101T000011-0000aaaa", ea_bus::Evento::Cancelar).unwrap();
    let estado_ultimo = bus.leer_estado(ultimo).unwrap().estado.as_str();

    let (s, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
    let ej = r["ejecuciones"].as_array().unwrap();
    let ids: Vec<&str> = ej.iter().map(|e| e["trabajo"].as_str().unwrap()).collect();
    let esperados: Vec<String> = (3..=12).rev().map(|n| format!("20260101T0000{n:02}-0000aaaa")).collect();
    assert_eq!(ids, esperados, "las diez más recientes, la más nueva primero, solo de esta rutina");
    assert_eq!(ej[0]["estado"], estado_ultimo);
    assert_eq!(ej[0]["version"], 2);
    assert_eq!(ej[0]["resultado"], "bloqueado");
    assert_eq!(ej[0]["duracion_s"], 42);
    assert_eq!(ej[1]["estado"], "cancelado");
    assert!(ej[1]["resultado"].is_null() && ej[1]["duracion_s"].is_null());
    assert_eq!(ej[2]["estado"], "en_cola");
    assert_eq!(ej[9]["version"], 1, "la v1 de las primeras");
    assert!(ej[0]["creado"].as_str().is_some_and(|c| !c.is_empty()));
    assert!(ej[0]["actualizado"].as_str().is_some_and(|c| !c.is_empty()));

    // La fila lleva solo la última, la misma que encabeza la ficha.
    let (_, l) = get(&f, "/api/rutinas").await;
    assert_eq!(l[0]["ultima"]["trabajo"], ultimo);
    assert_eq!(l[0]["ultima"]["estado"], estado_ultimo);
    assert_eq!(l[0]["ultima"]["resultado"], "bloqueado");

    // Solo lectura: ni trabajos nuevos ni cambios de estado.
    assert_eq!(bus.listar_trabajos().unwrap().len(), 14);
    assert_eq!(bus.leer_estado(ultimo).unwrap().estado.as_str(), estado_ultimo);
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
        equipo: None,
        asignado_a: None,
        revisor: None,
        worktree_propio: false,
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

    let id = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "Primera", tmp!())).unwrap();
    let (_, v) = get(&f, "/api/trabajos").await;
    assert_eq!(v[0]["id"], id.as_str());
    assert_eq!(v[0]["modo"], "mision");
    assert_eq!(v[0]["estado"], "en_cola");
    assert!(v[0]["resultado"].is_null());
    assert!(std::path::Path::new(v[0]["worktree"].as_str().unwrap()).ends_with(std::path::Path::new(tmp!()).file_name().unwrap()));

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
        std::path::Path::new(tmp!()),
        ea_bus::Ocupante { agente: "modelo-local".into(), trabajo: id.clone(), pid: 0, desde: "2026-10-14T10:00:00Z".into(), rol: None },
        std::time::Duration::from_millis(50),
    )
    .unwrap();
    let (_, sem) = get(&f, "/api/semaforos").await;
    assert_eq!(sem[0]["trabajo"], id.as_str());
    assert_eq!(sem[0]["agente"], "modelo-local");

    // Cancelado con el semáforo aún cogido: «cancelando…» hasta que se suelte.
    let cancelando = |l: &Value| l.as_array().unwrap().iter().find(|t| t["id"] == id.as_str()).unwrap()["cancelando"].clone();
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(cancelando(&l), json!(false));
    bus.aplicar(&id, ea_bus::Evento::Cancelar).unwrap();
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(cancelando(&l), json!(true));
    drop(_candado);
    let (_, l) = get(&f, "/api/trabajos").await;
    assert_eq!(cancelando(&l), json!(false));
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
    let a = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "A", tmp!())).unwrap();
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
    let b = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "B", tmp!())).unwrap();
    entrega(&b, true);
    let (s, _) = post(&f, &format!("/api/trabajos/{b}/reformular"), json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = post(&f, &format!("/api/trabajos/{b}/reformular"), json!({ "nota": "falta el caso vacío" })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    assert_eq!(bus.leer_orden(&b).unwrap().notas_revision, vec!["falta el caso vacío"]);

    // continuar y cancelar
    let c1 = bus.crear_trabajo(orden(ea_bus::Modo::Obra, "C", tmp!())).unwrap();
    entrega(&c1, false);
    let (s, r) = post(&f, &format!("/api/trabajos/{c1}/continuar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    let (s, r) = post(&f, &format!("/api/trabajos/{c1}/cancelar"), json!({})).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("cancelado")));

    // confirmar el paquete de corrección de una rutina en espera
    let r1 = bus.crear_trabajo(orden(ea_bus::Modo::Rutina, "R", tmp!())).unwrap();
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

/// Obra de tres fases entregada completa (las tres hechas), en revisión.
fn obra_completa(bus: &ea_bus::Bus) -> String {
    let fase = |n: u32| ea_bus::Fase {
        numero: n,
        titulo: format!("f{n}"),
        instrucciones: "i".into(),
        entregable: "e".into(),
        criterio: "true".into(),
        skills: vec![],
    };
    let o = ea_bus::Orden { criterio_exito: None, fases: vec![fase(1), fase(2), fase(3)], ..orden(ea_bus::Modo::Obra, "O", tmp!()) };
    let id = bus.crear_trabajo(o).unwrap();
    bus.aplicar(&id, ea_bus::Evento::Tomar).unwrap();
    let inf = ea_bus::Informe { fases_hechas: 3, obra_completa: true, ..ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Terminado) };
    bus.aplicar(&id, ea_bus::Evento::Entregar(inf)).unwrap();
    id
}

#[tokio::test]
async fn reformular_con_fase_fuera_de_rango_da_400() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let bus = ea_bus::Bus::abrir().unwrap();
    let o = obra_completa(&bus);
    let antes = bus.leer_estado(&o).unwrap();
    for fase in [0, 4] {
        let (s, r) = post(&f, &format!("/api/trabajos/{o}/reformular"), json!({ "nota": "rehaz", "fase": fase })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "fase {fase}: {r}");
    }
    // Una misión no tiene fases.
    let m = bus.crear_trabajo(orden(ea_bus::Modo::Mision, "M", tmp!())).unwrap();
    bus.aplicar(&m, ea_bus::Evento::Tomar).unwrap();
    bus.aplicar(&m, ea_bus::Evento::Entregar(ea_bus::Informe::nuevo(ea_bus::ResultadoInforme::Terminado))).unwrap();
    let (s, _) = post(&f, &format!("/api/trabajos/{m}/reformular"), json!({ "nota": "rehaz", "fase": 1 })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Nada escrito: ni notas ni estado.
    assert_eq!(bus.leer_estado(&o).unwrap(), antes);
    assert!(bus.leer_orden(&o).unwrap().notas_revision.is_empty());
    assert!(bus.leer_orden(&m).unwrap().notas_revision.is_empty());
    assert_eq!(bus.leer_estado(&m).unwrap().estado, ea_bus::EstadoTrabajo::EnRevision);
}

#[tokio::test]
async fn reformular_una_obra_con_fase_deja_la_fase_actual_en_ella() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let bus = ea_bus::Bus::abrir().unwrap();
    let o = obra_completa(&bus);
    let (s, r) = post(&f, &format!("/api/trabajos/{o}/reformular"), json!({ "nota": "rehaz la 2", "fase": 2 })).await;
    assert_eq!((s, r["estado"].as_str()), (StatusCode::OK, Some("en_cola")));
    let (_, j) = get(&f, &format!("/api/trabajos/{o}")).await;
    assert_eq!(j["estado"]["fase_actual"], json!(2));
    assert_eq!(j["orden"]["notas_revision"], json!(["rehaz la 2"]));
    // Sin fase, una obra completa rehace la última.
    let o2 = obra_completa(&bus);
    let (s, _) = post(&f, &format!("/api/trabajos/{o2}/reformular"), json!({ "nota": "retoca" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(bus.leer_estado(&o2).unwrap().fase_actual, Some(3));
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
    // Solo se borra para siempre una rutina retirada (en la papelera).
    ea_bus::retirar_rutina(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let retirada = ea_bus::ruta_retirada(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let id = proponer_accion(&f, "ea_eliminar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, lista) = get(&f, "/api/propuestas").await;
    assert_eq!(lista[0]["accion"]["riesgo"], "irreversible");

    let (s, e) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(e["error"].as_str().unwrap().contains("terminal"));
    assert!(retirada.exists());
    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista.as_array().unwrap().len(), 1, "sigue pendiente");

    // Rechazarla sí se puede desde la web.
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert_eq!((s, p["estado"].as_str()), (StatusCode::OK, Some("rechazada")));
    assert!(retirada.exists());
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

/// A1: el resultado de una acción dice qué produjo, para enlazarlo: el
/// trabajo y el id de su evento `accion_ejecutada` en el diario. Aceptar una
/// propuesta devuelve lo mismo en `resultado`; listarlas, no.
#[tokio::test]
async fn el_resultado_de_una_accion_trae_su_trabajo_y_su_evento() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let evento_de = |d: &Value, id: i64| d.as_array().unwrap().iter().find(|e| e["id"] == id).cloned();

    let cuerpo = json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (_, sim) = post(&f, "/api/acciones/simular", cuerpo).await;
    let (s, r) = post(
        &f,
        "/api/acciones/ejecutar",
        json!({ "herramienta": "ea_encolar_rutina", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let trabajo = r["trabajo"].as_str().unwrap().to_string();
    assert_eq!(r["datos"]["trabajo"], trabajo.as_str());
    let ev = r["evento"].as_i64().expect("trae el evento");
    let (_, d) = get(&f, "/api/diario").await;
    let e = evento_de(&d, ev).expect("el evento está en el diario");
    assert_eq!((e["accion"].as_str(), e["trabajo"].as_str()), (Some("accion_ejecutada"), Some(trabajo.as_str())));

    // La misma rutina, ahora por una propuesta aceptada en la web.
    let (s, _) = post(&f, &format!("/api/trabajos/{trabajo}/cancelar"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, ps) = get(&f, "/api/propuestas").await;
    assert!(ps.as_array().unwrap().iter().all(|p| p.get("resultado").is_none()), "listar no trae resultado");
    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!((s, p["ejecutada"]["estado"].as_str()), (StatusCode::OK, Some("ejecutada")), "{p}");
    let otro = p["resultado"]["trabajo"].as_str().expect("trae el trabajo").to_string();
    assert_ne!(otro, trabajo);
    let ev = p["resultado"]["evento"].as_i64().expect("trae el evento");
    let (_, d) = get(&f, "/api/diario").await;
    let e = evento_de(&d, ev).unwrap();
    assert_eq!((e["accion"].as_str(), e["trabajo"].as_str()), (Some("accion_ejecutada"), Some(otro.as_str())));

    // Rechazar no ejecuta nada: sin resultado.
    let (s, _) = post(&f, &format!("/api/trabajos/{otro}/cancelar"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (_, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert!(p.get("resultado").is_none(), "{p}");
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

    ea_bus::retirar_rutina(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
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
    assert!(ea_bus::ruta_retirada(&f.state.config.rutinas_dir, "revision-diaria").unwrap().exists());
}

fn evento_mcp(actor: &str, accion: &str, ok: bool) -> NewEvent {
    NewEvent {
        actor: actor.into(),
        action: accion.into(),
        trabajo: None,
        outcome: Some(if ok { EventOutcome::Ok } else { EventOutcome::Error }),
        duration_ms: Some(3),
        detail: Some(json!({ "args": {} }).to_string()),
    }
}

#[tokio::test]
async fn el_historial_de_una_herramienta_incluye_simulaciones_y_ejecuciones() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    {
        let store = f.state.store.lock().await;
        store.append_event(evento_mcp("claude-code", "mcp_encolar_rutina_simulada", true)).unwrap();
        store.append_event(evento_mcp("modelo-local", "mcp_encolar_rutina_denegada", false)).unwrap();
        // Ruido: otra herramienta, otra familia y un `accion_*` sin JSON.
        store.append_event(evento_mcp("claude-code", "mcp_registrar_rutina", true)).unwrap();
        store.append_event(evento_mcp("claude-code", "mcp_status", true)).unwrap();
        store
            .append_event(NewEvent { detail: Some("no es json".into()), ..evento_mcp("usuario", "accion_fallida", false) })
            .unwrap();
    }
    // Una propuesta (propuesta_encolada) que el usuario rechaza: el rechazo
    // solo trae el id de la propuesta.
    let id = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    let (s, _) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "rechazar" })).await;
    assert_eq!(s, StatusCode::OK);
    // Y una ejecución desde la consola (accion_ejecutada, canal web).
    let cuerpo = json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (_, sim) = post(&f, "/api/acciones/simular", cuerpo).await;
    let (s, _) = post(
        &f,
        "/api/acciones/ejecutar",
        json!({ "herramienta": "ea_encolar_rutina", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let (s, h) = get(&f, "/api/acciones/ea_encolar_rutina/historial").await;
    assert_eq!(s, StatusCode::OK);
    let h = h.as_array().unwrap();
    let acciones: Vec<&str> = h.iter().map(|e| e["accion"].as_str().unwrap()).collect();
    assert_eq!(
        acciones,
        [
            "accion_ejecutada",
            "propuesta_decidida",
            "propuesta_encolada",
            "mcp_encolar_rutina_denegada",
            "mcp_encolar_rutina_simulada",
        ],
        "lo más reciente primero, sin lo de otras herramientas"
    );
    assert_eq!(h[0]["canal"], "web");
    assert_eq!(h[0]["actor"], "usuario");
    assert_eq!(h[0]["resultado"], "ok");
    assert_eq!(h[1]["propuesta"], id);
    assert_eq!(h[2]["actor"], "claude-code");
    assert_eq!(h[3]["canal"], "mcp");
    assert_eq!(h[3]["resultado"], "error");
    // Cada fila lleva el id de su evento en el diario.
    let (_, d) = get(&f, "/api/diario").await;
    let ids: Vec<i64> = d.as_array().unwrap().iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert!(h.iter().all(|e| ids.contains(&e["id"].as_i64().unwrap())));

    // Las directas también tienen historial; y como mucho diez eventos.
    {
        let store = f.state.store.lock().await;
        for _ in 0..12 {
            store.append_event(evento_mcp("claude-code", "mcp_status", true)).unwrap();
        }
    }
    let (s, h) = get(&f, "/api/acciones/ea_status/historial").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.as_array().unwrap().len(), 10);
    assert!(h.as_array().unwrap().iter().all(|e| e["accion"] == "mcp_status"));

    let (s, _) = get(&f, "/api/acciones/ea_no_existe/historial").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_catalogo_guia_los_formularios_con_sugerencias_y_estados() {
    let f = montar();
    let propia = nota(&f, "claude-code", "Plan de la semana", "Repasar el backlog.");
    let ajena = nota(&f, "modelo-local", "Receta de lentejas", "Lentejas con chorizo.");
    let promovida = nota(&f, "modelo-local", "Hallazgo", "Ya compartido.");
    let (s, _) = post(&f, "/api/boveda/promover", json!({ "ruta": promovida })).await;
    assert_eq!(s, StatusCode::OK);

    let (s, cat) = get(&f, "/api/acciones").await;
    assert_eq!(s, StatusCode::OK);
    let campo = |h: &str, c: &str| {
        let h = cat.as_array().unwrap().iter().find(|x| x["nombre"] == h).unwrap();
        h["campos"].as_array().unwrap().iter().find(|x| x["nombre"] == c).unwrap().clone()
    };

    // La ruta de promover: las notas de los agentes que aún no se han promovido.
    let ruta = campo("ea_promover_nota", "ruta");
    let sug: Vec<&str> = ruta["sugerencias"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(sug.contains(&propia.as_str()) && sug.contains(&ajena.as_str()), "{sug:?}");
    assert!(!sug.contains(&promovida.as_str()), "una promovida no se vuelve a promover");
    assert!(sug.iter().all(|r| !r.starts_with("compartida/")));
    assert_eq!(ruta["opciones"], json!([]), "sigue siendo un campo libre");
    assert_eq!(campo("ea_promover_nota", "destino")["sugerencias"], json!([]));

    // El servicio: el estado de cada uno junto a la opción (sin systemctl, desconocido).
    let srv = campo("ea_reiniciar_servicio", "servicio");
    for o in srv["opciones"].as_array().unwrap() {
        let o = o.as_str().unwrap();
        assert_eq!(srv["estados"][o], if o == "ea-web" { "active" } else { "desconocido" }, "{o}");
    }

    // El slug: las rutinas registradas.
    assert_eq!(campo("ea_eliminar_rutina", "slug")["opciones"], json!(["revision-diaria"]));
    assert_eq!(campo("ea_encolar_rutina", "slug")["estados"], json!({}));
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
        json!({"pid": pid, "sessionId": "x", "cwd": tmp!(), "name": nombre, "status": "busy",
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
        json!({"pid": pid, "sessionId": id, "cwd": tmp!(), "name": id, "status": "idle",
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
    ea_plataforma::fs::enlazar(&fuera, &mem.join("enlace.md")).unwrap();
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
    // Como `Path`, no como texto: en Windows la barra y la contrabarra valen igual.
    assert_eq!(std::path::PathBuf::from(v["ruta"].as_str().unwrap()), mem.join("refactor.md"));
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
    ea_plataforma::fs::enlazar(&fuera, &mem.join("enlace.md")).unwrap();
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
    assert!(std::path::Path::new(v["absoluta"].as_str().unwrap()).ends_with("boveda/modelo-local/receta.md"));
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
    assert_eq!(ids, ["roles", "agentes", "modelos", "agente", "orquestador", "red", "rutas", "web", "biblioteca"]);
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
    // roles (017): el ejecutor se cambia desde aquí; un agente no ocupa los dos
    let (s, _) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "agentes.codex.bin": "/tmp/x" } })).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "roles.ejecutor": "codex" } })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["aplicados"][0]["clave"], "roles.ejecutor");
    let version = v["ajustes"]["version"].clone();
    let (s, v) = post(&f, "/api/ajustes", json!({ "version": version, "cambios": { "roles.planificador": "codex" } })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let cuerpo = json!({ "version": version, "cambios": { "web.sondeo_ms": 5000 } });
    let (s, _) = enviar(&f, Method::POST, "/api/ajustes", "localhost:8092", Some(cuerpo.clone()), false).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let (s, _) = enviar(&f, Method::POST, "/api/ajustes", "evil.example", Some(cuerpo), true).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, v) = get(&f, "/api/ajustes").await;
    assert_eq!(clave_de(&v, "agente.techo_topes.turnos")["valor"], 20);
    assert_eq!(clave_de(&v, "agente.sandbox")["valor"], true);
}

#[tokio::test]
async fn los_agentes_dicen_su_rol_y_el_cambio_de_ejecutor_se_ve_en_el_estado() {
    let f = montar();
    // Un endpoint cerrado: la prueba no pregunta al llama-server de verdad.
    let fichero = f.state.config.ajustes.fichero();
    std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
    std::fs::write(&fichero, "[modelos.local]\nbase_url = \"http://127.0.0.1:9/v1\"\n").unwrap();
    let (s, v) = get(&f, "/api/agentes").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ag = |v: &Value, id: &str| v["agentes"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    let ids: Vec<&str> = v["agentes"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ea_ajustes::IDS_AGENTES);
    assert_eq!(ag(&v, "qwen-code")["rol"], "ejecutor");
    assert_eq!(ag(&v, "qwen-code")["modelo"]["nombre"], "qwen3-coder-30b");
    assert_eq!(ag(&v, "qwen-code")["modelo_accesible"], false);
    assert_eq!(ag(&v, "claude-code")["rol"], "planificador");
    assert_eq!(ag(&v, "claude-code")["modelo_accesible"], Value::Null);
    assert_eq!(ag(&v, "antigravity")["admite"], json!(["planificador"]));
    assert_eq!(v["fijados"], json!({}));

    let (s, r) = post(&f, "/api/ajustes", json!({ "version": v["version"], "cambios": { "roles.ejecutor": "codex" } })).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (_, v) = get(&f, "/api/agentes").await;
    assert_eq!(ag(&v, "codex")["rol"], "ejecutor");
    assert_eq!(ag(&v, "qwen-code")["rol"], Value::Null);
    let (_, e) = get(&f, "/api/estado").await;
    let ejecutor = e["agentes"].as_array().unwrap().iter().find(|a| a["rol"] == "ejecutor").unwrap().clone();
    assert_eq!((ejecutor["nombre"].clone(), ejecutor["agente"].clone()), (json!("modelo-local"), json!("codex")));
    assert_eq!((ejecutor["agente_nombre"].clone(), ejecutor["modelo"].clone()), (json!("Codex"), json!("qwen3-coder-30b")));
    let (_, d) = get(&f, "/api/diario?accion=ajustes_").await;
    assert!(d.to_string().contains("roles.ejecutor"), "{d}");
}

// ------------------------------------------------------- crear una misión

/// Un repo git de verdad dentro del directorio temporal de la prueba, con
/// un commit inicial (el ejecutor arranca con `rev-parse HEAD`).
fn repo_git(f: &Fixture) -> std::path::PathBuf {
    let wt = repo_git_vacio(f, "repo");
    let ok = std::process::Command::new("git")
        .arg("-C")
        .arg(&wt)
        .args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"])
        .args(["commit", "-q", "--allow-empty", "-m", "inicial"])
        .status()
        .unwrap()
        .success();
    assert!(ok, "git commit");
    wt
}

/// Un repo recién iniciado, sin ningún commit.
fn repo_git_vacio(f: &Fixture, nombre: &str) -> std::path::PathBuf {
    let wt = f._dir.path().join(nombre);
    std::fs::create_dir_all(&wt).unwrap();
    let ok = std::process::Command::new("git").arg("init").arg("-q").arg(&wt).status().unwrap().success();
    assert!(ok, "git init");
    ea_plataforma::rutas::canonica(&wt).unwrap()
}

fn mision(wt: &str) -> Value {
    json!({
        "titulo": "Añadir un saludo",
        "worktree": wt,
        "objetivo": "Crea hola.txt con un saludo.",
        "criterio": "test -f hola.txt",
        "topes": { "iteraciones": 3, "tiempo_s": 999_999 },
    })
}

#[tokio::test]
async fn crear_una_mision_desde_la_web_la_encola() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = repo_git(&f);
    // El formulario da los modos, el techo y los topes por defecto.
    let (s, form) = get(&f, "/api/trabajos/nuevo").await;
    assert_eq!(s, StatusCode::OK, "{form}");
    assert_eq!(form["modos"], json!(["mision", "obra"]));
    let techo_tiempo = form["techo"]["tiempo_s"].as_u64().unwrap();
    assert!(form["defecto"]["iteraciones"].as_u64().unwrap() <= form["techo"]["iteraciones"].as_u64().unwrap());

    // Sin canonicalizar: la orden lleva la ruta canónica.
    let (s, r) = post(&f, "/api/trabajos", mision(&format!("{}/./", wt.display()))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["estado"], "en_cola");
    let id = r["id"].as_str().unwrap().to_string();

    let (_, colas) = get(&f, "/api/colas").await;
    let ml = colas["modelo-local"].as_array().unwrap();
    assert!(ml.iter().any(|e| e["trabajo"] == id.as_str()), "{colas}");

    let (s, d) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    let o = &d["orden"];
    assert_eq!(o["creada_por"], "usuario");
    assert_eq!(o["modo"], "mision");
    assert_eq!(o["worktree"], wt.to_str().unwrap());
    assert_eq!(o["criterio_exito"], "test -f hola.txt");
    assert!(o.get("skills").is_none(), "sin skills: {o}");
    assert_eq!(o["topes"]["iteraciones"], 3);
    // Lo que pasa del techo se recorta, como hace el ejecutor.
    assert_eq!(o["topes"]["tiempo_s"].as_u64().unwrap(), techo_tiempo);
    // El criterio no se ha ejecutado: no hay informe.
    assert!(d["informe"].is_null());

    // El diario apunta la creación con el usuario como actor.
    let (_, ev) = get(&f, "/api/diario").await;
    let creado = ev.as_array().unwrap().iter().find(|e| e["accion"] == "bus_crear").expect("bus_crear en el diario");
    assert_eq!(creado["actor"], "usuario");

    // Y el worktree queda sugerido para la próxima.
    let (_, form) = get(&f, "/api/trabajos/nuevo").await;
    assert!(form["worktrees"].as_array().unwrap().iter().any(|w| w == wt.to_str().unwrap()), "{form}");
}

#[tokio::test]
async fn crear_sin_criterio_o_worktree_invalido_da_400_y_no_escribe() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = repo_git(&f);
    let wt = wt.to_str().unwrap();
    let sin_repo = f._dir.path().join("sin-repo");
    std::fs::create_dir_all(&sin_repo).unwrap();
    let fichero = f._dir.path().join("fichero.txt");
    std::fs::write(&fichero, "x").unwrap();
    let con = |campo: &str, valor: Value| {
        let mut m = mision(wt);
        m[campo] = valor;
        m
    };
    let sin = |campo: &str| {
        let mut m = mision(wt);
        m.as_object_mut().unwrap().remove(campo);
        m
    };
    let casos = [
        con("criterio", json!("")),
        con("criterio", json!("   ")),
        sin("criterio"),
        con("criterio", json!("x".repeat(2_001))),
        con("worktree", json!("")),
        con("worktree", json!("relativo/a/algo")),
        con("worktree", json!(concat!(tmp!(), "/no/existe/de/ninguna/manera"))),
        con("worktree", json!(sin_repo.to_str().unwrap())),
        con("worktree", json!(fichero.to_str().unwrap())),
        sin("worktree"),
        con("titulo", json!("")),
        con("titulo", json!("dos\nlineas")),
        con("titulo", json!("t".repeat(201))),
        con("objetivo", json!("")),
        con("modo", json!("obra")),
        con("modo", json!("rutina")),
        con("skills", json!(["git"])),
        con("creada_por", json!("claude-code")),
        con("topes", json!({ "iteraciones": 0 })),
        con("topes", json!({ "turnos": 99 })),
        json!("no es un objeto"),
    ];
    for c in casos {
        let (s, r) = post(&f, "/api/trabajos", c.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{c} -> {r}");
        assert!(r["error"].as_str().is_some_and(|e| !e.is_empty()), "mensaje: {r}");
    }
    // Un JSON roto también es 400.
    let (s, _) = enviar(&f, Method::POST, "/api/trabajos", "localhost", None, true).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Nada en el bus: ni trabajos, ni colas, ni eventos de creación.
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    let (_, colas) = get(&f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().map(Vec::len), Some(0), "{colas}");
    let (_, ev) = get(&f, "/api/diario").await;
    assert!(!acciones_del_diario(&ev).iter().any(|a| a == "bus_crear"));
    let trabajos = std::fs::read_dir(f._dir.path().join("estado/trabajos")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(trabajos, 0);
}

#[tokio::test]
async fn crear_exige_json_host_local_y_clave() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = repo_git(&f);
    let cuerpo = mision(wt.to_str().unwrap());
    let clave = f.state.clave.texto().to_string();
    // Sin clave o con una mala: 403.
    for c in [None, Some("mala")] {
        let (s, t) = enviar_con_clave(&f, Method::POST, "/api/trabajos", "localhost", Some(cuerpo.clone()), true, c).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{c:?}: {t}");
    }
    // Con la clave pero sin JSON: 415.
    let (s, _) =
        enviar_con_clave(&f, Method::POST, "/api/trabajos", "localhost", Some(cuerpo.clone()), false, Some(&clave)).await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // Con la clave desde otro host: 403.
    let (s, _) =
        enviar_con_clave(&f, Method::POST, "/api/trabajos", "evil.example", Some(cuerpo.clone()), true, Some(&clave)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    // Con todo en regla, se crea.
    let (s, r) =
        enviar_con_clave(&f, Method::POST, "/api/trabajos", "127.0.0.1:8092", Some(cuerpo), true, Some(&clave)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
}

// --------------------------------------------------------- crear una obra

fn fase(n: u32) -> Value {
    json!({
        "titulo": format!("Fase {n}"),
        "instrucciones": format!("Crea f{n}.txt."),
        "entregable": format!("f{n}.txt"),
        "criterio": format!("test -f f{n}.txt"),
    })
}

fn obra(wt: &str) -> Value {
    json!({
        "modo": "obra",
        "titulo": "Tres ficheros",
        "worktree": wt,
        "objetivo": "Crea f1.txt, f2.txt y f3.txt, uno por fase.",
        "fases": [fase(1), fase(2), fase(3)],
        "revisar_cada": 2,
        "topes": { "iteraciones": 2, "tiempo_s": 999_999 },
    })
}

/// Nada en el bus: ni trabajos, ni colas, ni eventos de creación.
async fn bus_vacio(f: &Fixture) {
    let (_, t) = get(f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
    let (_, colas) = get(f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().map(Vec::len), Some(0), "{colas}");
    let (_, ev) = get(f, "/api/diario").await;
    assert!(!acciones_del_diario(&ev).iter().any(|a| a == "bus_crear"));
    let trabajos = std::fs::read_dir(f._dir.path().join("estado/trabajos")).map(|d| d.count()).unwrap_or(0);
    assert_eq!(trabajos, 0);
}

#[tokio::test]
async fn crear_una_obra_valida_cada_fase() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = repo_git(&f);
    let wt = wt.to_str().unwrap();
    let con = |campo: &str, valor: Value| {
        let mut o = obra(wt);
        o[campo] = valor;
        o
    };
    let sin = |campo: &str| {
        let mut o = obra(wt);
        o.as_object_mut().unwrap().remove(campo);
        o
    };
    let fase_con = |campo: &str, valor: Value| {
        let mut o = obra(wt);
        o["fases"][1][campo] = valor;
        o
    };
    let fase_sin = |campo: &str| {
        let mut o = obra(wt);
        o["fases"][1].as_object_mut().unwrap().remove(campo);
        o
    };
    let casos = [
        // Una fase sin criterio: la rechaza el bus antes de escribir nada.
        (fase_con("criterio", json!("")), "la fase 2 no tiene criterio"),
        (fase_con("criterio", json!("   ")), "la fase 2 no tiene criterio"),
        (fase_sin("criterio"), "la fase 2 no tiene criterio"),
        // revisar_cada se elige expresamente: sin él no hay valor por defecto.
        (sin("revisar_cada"), "revisar_cada"),
        (con("revisar_cada", json!(0)), "revisar_cada"),
        (con("revisar_cada", json!(4)), "revisar_cada"),
        (con("revisar_cada", json!(-1)), "revisar_cada"),
        (con("revisar_cada", json!("2")), "revisar_cada"),
        (sin("fases"), "al menos una fase"),
        (con("fases", json!([])), "al menos una fase"),
        (con("fases", json!((1..=21).map(fase).collect::<Vec<_>>())), "como mucho"),
        (con("criterio", json!("test -f todo")), "cada fase"),
        (fase_con("titulo", json!("")), "fase 2: titulo"),
        (fase_con("titulo", json!("dos\nlineas")), "fase 2: titulo"),
        (fase_con("instrucciones", json!(" ")), "fase 2: instrucciones"),
        (fase_sin("entregable"), "entregable"),
        (fase_con("criterio", json!("x".repeat(2_001))), "fase 2: criterio"),
        // Ni skills ni número por fase: no se eligen desde la web.
        (fase_con("skills", json!(["git"])), "skills"),
        (fase_con("numero", json!(7)), "numero"),
        (con("fases", json!("no es una lista")), "petición inválida"),
        (con("skills", json!(["git"])), "skills"),
        // Una misión no lleva fases ni revisar_cada.
        (con("modo", json!("mision")), "modo 'obra'"),
    ];
    for (c, motivo) in casos {
        let (s, r) = post(&f, "/api/trabajos", c.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{c} -> {r}");
        assert!(r["error"].as_str().is_some_and(|e| e.contains(motivo)), "{motivo}: {r}");
    }
    bus_vacio(&f).await;

    // Una obra válida: 201, en cola, en la fase 1, con las fases numeradas
    // por su orden y sin criterio general.
    let (s, r) = post(&f, "/api/trabajos", obra(wt)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["estado"], "en_cola");
    let id = r["id"].as_str().unwrap().to_string();
    let (_, d) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(d["estado"]["fase_actual"], 1, "{d}");
    let o = &d["orden"];
    assert_eq!(o["modo"], "obra");
    assert_eq!(o["creada_por"], "usuario");
    assert_eq!(o["revisar_cada"], 2);
    assert!(o["criterio_exito"].is_null(), "{o}");
    assert!(o.get("skills").is_none(), "sin skills: {o}");
    assert_eq!(o["topes"]["iteraciones"], 2);
    let fases = o["fases"].as_array().unwrap();
    assert_eq!(fases.len(), 3);
    for (i, fz) in fases.iter().enumerate() {
        assert_eq!(fz["numero"], i + 1);
        assert_eq!(fz["criterio"], format!("test -f f{}.txt", i + 1));
        assert!(fz.get("skills").is_none(), "{fz}");
    }
    // Lo mismo en el orden.json del bus.
    let disco: Value = serde_json::from_str(
        &std::fs::read_to_string(f._dir.path().join("estado/trabajos").join(&id).join("orden.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(disco["fases"], o["fases"]);
    // El criterio no se ha ejecutado y el diario apunta al usuario.
    assert!(d["informe"].is_null());
    let (_, ev) = get(&f, "/api/diario").await;
    let creado = ev.as_array().unwrap().iter().find(|e| e["accion"] == "bus_crear").expect("bus_crear en el diario");
    assert_eq!(creado["actor"], "usuario");
    let (_, lista) = get(&f, "/api/trabajos").await;
    assert_eq!(lista[0]["fases_total"], 3);

    // `revisar_cada: null` (solo al final) es una elección válida.
    let (s, r) = post(&f, "/api/trabajos", con("revisar_cada", Value::Null)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let (_, d) = get(&f, &format!("/api/trabajos/{}", r["id"].as_str().unwrap())).await;
    assert!(d["orden"]["revisar_cada"].is_null());
}

#[tokio::test]
async fn crear_en_un_repo_sin_commits_da_400() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let vacio = repo_git_vacio(&f, "sin-commits");
    let vacio = vacio.to_str().unwrap();
    for cuerpo in [mision(vacio), obra(vacio)] {
        let (s, r) = post(&f, "/api/trabajos", cuerpo).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
        assert!(r["error"].as_str().unwrap().contains("el repo no tiene ningún commit"), "{r}");
    }
    bus_vacio(&f).await;
}

// ------------------------------------------- registrar rutinas desde la web

/// Lo que hay registrado y en `rutinas_dir` de una rutina: (versión, creada_por, fichero).
async fn registro(f: &Fixture, slug: &str) -> (Option<(i64, String)>, Option<String>) {
    let fila = f.state.store.lock().await.ultima_rutina(slug).unwrap().map(|r| (r.version, r.creada_por));
    let fichero = std::fs::read_to_string(f.state.config.rutinas_dir.join(format!("{slug}.toml"))).ok();
    (fila, fichero)
}

async fn registros_en_el_diario(f: &Fixture) -> Vec<ea_store::Event> {
    let todos = f.state.store.lock().await.list_events(&ea_store::EventFilter { limit: Some(500), ..Default::default() }).unwrap();
    todos.into_iter().filter(|e| e.action == "rutina_registrada").collect()
}

#[tokio::test]
async fn registrar_desde_la_web_exige_version_mayor_y_toml_valido() {
    let f = montar();
    let antes = registro(&f, "revision-diaria").await;
    for (toml, motivo) in [
        (RUTINA_BUS.to_string(), "la nueva debe ser mayor"),
        (RUTINA_BUS.replace("version = 1", "version = 0"), "la versión empieza en 1"),
        (RUTINA_BUS.replace("version = 1", "version = 2").replace(concat!("\"", tmp!(), "\""), "\"tmp\""), "worktree debe ser una ruta absoluta"),
        (RUTINA_BUS.replace("version = 1", "version = 2").replace("test -f {salida}", " "), "falta criterio_exito"),
        ("[rutina]\nslug = ".into(), "rutina inválida"),
        (format!("{}#{}", RUTINA_BUS.replace("version = 1", "version = 2"), "x".repeat(ea_bus::MAX_DEFINICION)), "demasiado larga"),
    ] {
        let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": toml })).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{motivo}: {v}");
        assert!(v["error"].as_str().unwrap().contains(motivo), "{motivo}: {v}");
    }
    // Cuerpos que no son {toml}: 400 también, sin escribir.
    for cuerpo in [json!({}), json!({ "toml": RUTINA_BUS, "creada_por": "otro" }), json!({ "toml": 3 })] {
        let (s, v) = post(&f, "/api/rutinas/registrar", cuerpo).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    }
    assert_eq!(registro(&f, "revision-diaria").await, antes, "nada escrito");
    assert!(registros_en_el_diario(&f).await.is_empty());

    // La versión 2 sí: como `ea rutina registrar` (SQLite + rutinas_dir), con el usuario de actor.
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2").replace("test -f {salida}", "test -s {salida}");
    let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": v2 })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v, json!({ "rutina": "revision-diaria", "version": 2, "anterior": 1 }));
    assert_eq!(registro(&f, "revision-diaria").await, (Some((2, "usuario".into())), Some(v2.clone())));
    let (_, d) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(d["version"], 2);
    let ev = registros_en_el_diario(&f).await;
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].actor, "usuario");
    let det: Value = serde_json::from_str(ev[0].detail.as_deref().unwrap()).unwrap();
    assert_eq!((det["canal"].as_str(), det["version"].as_i64(), det["anterior"].as_i64()), (Some("web"), Some(2), Some(1)));
    // Registrar no lanza nada.
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
}

#[tokio::test]
async fn validar_no_registra_nada() {
    let f = montar();
    let antes = registro(&f, "revision-diaria").await;
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2").replace("test -f {salida}", "test -s {salida}");
    let (s, v) = post(&f, "/api/rutinas/validar", json!({ "toml": v2 })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!((v["rutina"].as_str(), v["version"].as_u64()), (Some("revision-diaria"), Some(2)));
    assert_eq!(v["anterior"]["version"], 1);
    assert_eq!(v["anterior"]["definicion"], RUTINA_BUS);
    // La vista previa de R1 con las entradas por defecto.
    assert_eq!(v["previa"]["criterio"], "test -s informe.md");
    assert!(v["previa"]["objetivo"].as_str().unwrap().contains("revisa y escribe informe.md"));
    // Los mismos errores que registrar.
    let (s, v) = post(&f, "/api/rutinas/validar", json!({ "toml": RUTINA_BUS })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("la nueva debe ser mayor"), "{v}");
    // Una nueva: sin anterior.
    let nueva = RUTINA_BUS.replace("revision-diaria", "otra");
    let (s, v) = post(&f, "/api/rutinas/validar", json!({ "toml": nueva })).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["anterior"].is_null());

    assert_eq!(registro(&f, "revision-diaria").await, antes);
    assert_eq!(registro(&f, "otra").await, (None, None));
    assert!(registros_en_el_diario(&f).await.is_empty());
    let (_, t) = get(&f, "/api/trabajos").await;
    assert_eq!(t.as_array().map(Vec::len), Some(0), "{t}");
}

#[tokio::test]
async fn crear_una_rutina_nueva_desde_la_web() {
    let f = montar();
    let toml = concat!("# Una rutina nueva\n[rutina]\nslug = \"nueva\"\nversion = 1\ndescripcion = \"De prueba\"\nworktree = \"", tmp!(), "\"\ncriterio_exito = \"true\"\n\n[[pasos]]\ntexto = \"nada\"\n");
    let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": toml })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v, json!({ "rutina": "nueva", "version": 1, "anterior": null }));
    assert_eq!(registro(&f, "nueva").await, (Some((1, "usuario".into())), Some(toml.into())));
    let (_, l) = get(&f, "/api/rutinas").await;
    assert!(l.as_array().unwrap().iter().any(|r| r["slug"] == "nueva" && r["descripcion"] == "De prueba"), "{l}");
    let (s, d) = get(&f, "/api/rutinas/nueva").await;
    assert_eq!((s, d["contenido"].as_str()), (StatusCode::OK, Some(toml)));
    // Y se puede lanzar como cualquier otra.
    let _g = aislar_bus(&f).await;
    let (s, v) = post(&f, "/api/rutinas/nueva/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
}

#[tokio::test]
async fn validar_y_registrar_sin_clave_dan_403() {
    let f = montar();
    let antes = registro(&f, "revision-diaria").await;
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2");
    for ruta in ["/api/rutinas/validar", "/api/rutinas/registrar"] {
        for clave in [None, Some("mala")] {
            let (s, t) =
                enviar_con_clave(&f, Method::POST, ruta, "localhost:8092", Some(json!({ "toml": v2 })), true, clave).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{ruta} con {clave:?}: {t}");
        }
        // Con la clave pero sin JSON, tampoco.
        let (s, _) = enviar(&f, Method::POST, ruta, "localhost:8092", Some(json!({ "toml": v2 })), false).await;
        assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{ruta}");
    }
    assert_eq!(registro(&f, "revision-diaria").await, antes);
    assert!(registros_en_el_diario(&f).await.is_empty());
    // Las rutas fijas no tapan la ficha de una rutina que se llame así.
    let (s, _) = get(&f, "/api/rutinas/validar").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ------------------------------------------------- ficha completa (T6, H18)

/// `git` en `wt` con el índice que se pida (para las instantáneas sin tocar
/// el índice del repo, como hace el ejecutor). Devuelve stdout recortado.
fn git_en(wt: &std::path::Path, indice: Option<&std::path::Path>, args: &[&str]) -> String {
    let mut c = std::process::Command::new("git");
    c.arg("-C").arg(wt).args(["-c", "user.name=prueba", "-c", "user.email=prueba@localhost", "-c", "commit.gpgsign=false"]);
    if let Some(i) = indice {
        c.env("GIT_INDEX_FILE", i);
    }
    let o = c.args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Instantánea del árbol de trabajo con un índice aparte, como al cerrar una fase.
fn instantanea(wt: &std::path::Path) -> String {
    let idx = wt.parent().unwrap().join("indice-instantanea");
    let _ = std::fs::remove_file(&idx);
    git_en(wt, Some(&idx), &["add", "-A"]);
    git_en(wt, Some(&idx), &["write-tree"])
}

#[tokio::test]
async fn diff_de_una_fase_usa_las_instantaneas() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = f._dir.path().join("obra");
    std::fs::create_dir_all(&wt).unwrap();
    git_en(&wt, None, &["init", "-q"]);
    std::fs::write(wt.join("base.txt"), "base\n").unwrap();
    git_en(&wt, None, &["add", "-A"]);
    git_en(&wt, None, &["commit", "-q", "-m", "inicio"]);
    let inicio = git_en(&wt, None, &["rev-parse", "HEAD"]);

    let fase = |n: u32| -> ea_bus::Fase {
        serde_json::from_value(json!({
            "numero": n, "titulo": format!("fase {n}"), "instrucciones": format!("haz la {n}"),
            "entregable": format!("f{n}.txt"), "criterio": format!("test -f f{n}.txt"),
        }))
        .unwrap()
    };
    let mut o = orden(ea_bus::Modo::Obra, "Obra en tres fases", wt.to_str().unwrap());
    o.criterio_exito = None;
    o.fases = vec![fase(1), fase(2), fase(3)];
    let store = f.state.store.lock().await.duplicar().unwrap();
    let bus = ea_bus::Bus::abrir().unwrap().con_store(store);
    let id = bus.crear_trabajo(o).unwrap();

    // La obra cierra las fases 1 y 2 con su instantánea; la 3 sigue pendiente.
    std::fs::write(wt.join("f1.txt"), "uno\n").unwrap();
    let a1 = instantanea(&wt);
    bus.registrar_fase(&id, 1, "hecha", &json!({ "diff_stat": " f1.txt | 1 +", "arbol": a1 })).unwrap();
    std::fs::write(wt.join("f2.txt"), "dos\n").unwrap();
    std::fs::write(wt.join("base.txt"), "base cambiada\n").unwrap();
    let a2 = instantanea(&wt);
    bus.registrar_fase(&id, 2, "hecha", &json!({ "diff_stat": " 2 files changed", "arbol": a2 })).unwrap();

    // Sin informe todavía: la fase 2 ya tiene base (la instantánea de la 1);
    // la 1 no (le falta commit_inicio) y lo dice con un 404 claro.
    let (s, d) = get(&f, &format!("/api/trabajos/{id}/diff?fase=2")).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert!(d["commit_inicio"].is_null());
    let (s, e) = get(&f, &format!("/api/trabajos/{id}/diff?fase=1")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(e["error"].as_str().unwrap().contains("commit_inicio"), "{e}");

    bus.escribir_informe(
        &id,
        &serde_json::from_value(json!({ "resultado": "terminado", "commit_inicio": inicio })).unwrap(),
    )
    .unwrap();

    let (s, d1) = get(&f, &format!("/api/trabajos/{id}/diff?fase=1")).await;
    assert_eq!(s, StatusCode::OK, "{d1}");
    let t1 = d1["diff"].as_str().unwrap();
    assert_eq!((d1["fase"].as_u64(), d1["truncado"].as_bool()), (Some(1), Some(false)));
    assert!(t1.contains("f1.txt") && t1.contains("+uno"), "{t1}");
    assert!(!t1.contains("f2.txt") && !t1.contains("base cambiada"), "solo lo de la fase 1: {t1}");

    let (_, d2) = get(&f, &format!("/api/trabajos/{id}/diff?fase=2")).await;
    let t2 = d2["diff"].as_str().unwrap();
    assert!(t2.contains("f2.txt") && t2.contains("+base cambiada"), "{t2}");
    assert!(!t2.contains("f1.txt"), "solo lo de la fase 2: {t2}");

    // Sin instantánea: 404 con el motivo. Fase 0 o no entera: 400.
    let (s, e) = get(&f, &format!("/api/trabajos/{id}/diff?fase=3")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(e["error"].as_str().unwrap().contains("no hay instantánea de árbol de la fase 3"), "{e}");
    let (s, _) = get(&f, &format!("/api/trabajos/{id}/diff?fase=0")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&f, &format!("/api/trabajos/{id}/diff?fase=dos")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = get(&f, "/api/trabajos/no-existe/diff?fase=1").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Sin fase, el diff completo de siempre.
    let (s, d) = get(&f, &format!("/api/trabajos/{id}/diff")).await;
    assert_eq!((s, d["fase"].is_null()), (StatusCode::OK, true));
    assert!(d["diff"].as_str().unwrap().contains("f2.txt"));

    // La ficha: estado de cada fase, si tiene diff y el resumen del agente.
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    let fases = j["fases"].as_array().unwrap();
    assert_eq!(fases.len(), 3);
    assert_eq!((fases[0]["estado"].as_str(), fases[0]["instantanea"].as_bool()), (Some("hecha"), Some(true)));
    assert_eq!(fases[1]["diff_stat"], " 2 files changed");
    assert_eq!((fases[2]["estado"].as_str(), fases[2]["instantanea"].as_bool()), (Some("pendiente"), Some(false)));
    assert_eq!(j["orden"]["fases"][2]["criterio"], "test -f f3.txt");
    assert_eq!(j["orden"]["creada_por"], "test");
    assert!(j["resumen_agente"].is_null());
    let dir = bus.raiz().join("trabajos").join(&id);
    std::fs::write(dir.join("salida-qwen.json"), r#"[{"type":"result","result":"  hecho: f1 y f2  "}]"#).unwrap();
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(j["resumen_agente"], "hecho: f1 y f2");

    // Todo esto es lectura: el trabajo sigue donde estaba.
    assert_eq!(j["estado"]["estado"], "en_cola");
}

#[tokio::test]
async fn las_versiones_de_una_rutina_se_listan_y_se_leen() {
    let f = montar();
    // v1 la deja `montar` (creada por «test»); v2 se registra desde la web.
    let v2 = RUTINA_BUS.replace("version = 1", "version = 2").replace("revisa y", "revisa con calma y");
    let (s, v) = post(&f, "/api/rutinas/registrar", json!({ "toml": v2 })).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");

    let (s, l) = get(&f, "/api/rutinas/revision-diaria/versiones").await;
    assert_eq!(s, StatusCode::OK, "{l}");
    let l = l.as_array().unwrap();
    let resumen: Vec<_> =
        l.iter().map(|v| (v["version"].as_i64().unwrap(), v["creada_por"].as_str().unwrap())).collect();
    assert_eq!(resumen, vec![(2, "usuario"), (1, "test")]);
    assert!(l.iter().all(|v| v.get("contenido").is_none() && v.get("definicion").is_none()), "{l:?}");
    assert_eq!(l[1]["creada_en"], "2026-01-01T00:00:00Z");

    for (n, toml) in [(1, RUTINA_BUS.to_string()), (2, v2.clone())] {
        let (s, d) = get(&f, &format!("/api/rutinas/revision-diaria/versiones/{n}")).await;
        assert_eq!(s, StatusCode::OK, "{d}");
        assert_eq!((d["version"].as_i64(), d["contenido"].as_str()), (Some(n), Some(toml.as_str())));
        assert_eq!(d["slug"], "revision-diaria");
    }

    // Lo que no existe da 404 con un mensaje claro; lo mal formado, 400.
    let (s, e) = get(&f, "/api/rutinas/revision-diaria/versiones/3").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(e.to_string().contains("no tiene versión 3"), "{e}");
    let (s, _) = get(&f, "/api/rutinas/no-existe/versiones").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/no-existe/versiones/1").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/revision-diaria/versiones/dos").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Las rutas fijas de R5 no tapan estas ni la ficha.
    let (s, _) = get(&f, "/api/rutinas/validar/versiones").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
}

// ------------------------------------------- programar una rutina (R4)

/// Sin `sh` (Windows) no hay fakes de `systemctl`: órdenes que no existen,
/// y las pruebas que dependen de ellas se compilan solo en Unix.
#[cfg(not(unix))]
fn temporizadores_falsos(dir: &std::path::Path) -> ea_acciones::Temporizadores {
    let no = || vec!["orden-inexistente-para-las-pruebas".to_string()];
    ea_acciones::Temporizadores::systemd(dir.join("systemd-user"), no(), no())
}

/// `systemctl` y `systemd-analyze` falsos para los timers de las rutinas:
/// nunca el `~/.config/systemd/user` real ni el `systemctl` de verdad. Las
/// unidades van a `<dir>/systemd-user`; las órdenes, a `<dir>/ordenes.log`.
#[cfg(unix)]
fn temporizadores_falsos(dir: &std::path::Path) -> ea_acciones::Temporizadores {
    let log = dir.join("ordenes.log");
    let unidades = dir.join("systemd-user");
    let sc = dir.join("systemctl-falso.sh");
    std::fs::write(
        &sc,
        format!(
            "printf '%s\\n' systemctl \"$@\" >> '{log}'\n\
             if [ \"$2\" = is-active ]; then\n\
               if [ -f '{u}/'\"$3\" ]; then echo active; else echo inactive; exit 3; fi\n\
             fi\n",
            log = log.display(),
            u = unidades.display()
        ),
    )
    .unwrap();
    let sa = dir.join("systemd-analyze-falso.sh");
    std::fs::write(
        &sa,
        "case \"$4\" in *malo*) echo \"Failed to parse calendar specification '$4'\" >&2; exit 1;; esac\n\
         echo \"Normalized form: $4\"\n\
         echo '    Next elapse: Thu 2026-10-22 08:00:00 CEST'\n\
         echo '   Iteration #2: Fri 2026-10-23 08:00:00 CEST'\n\
         echo '   Iteration #3: Sat 2026-10-24 08:00:00 CEST'\n",
    )
    .unwrap();
    let sh = |p: &std::path::Path| vec!["sh".to_string(), p.display().to_string()];
    ea_acciones::Temporizadores::systemd(unidades, sh(&sc), sh(&sa))
}

#[cfg(unix)]
#[tokio::test]
async fn programar_desde_la_web_pide_simulacion_y_revalida() {
    let f = montar();
    let timer = f._dir.path().join("systemd-user/ea-rutina@revision-diaria.timer");

    // La consola ofrece la herramienta con las rutinas registradas como dominio.
    let (_, cat) = get(&f, "/api/acciones").await;
    let h = cat.as_array().unwrap().iter().find(|h| h["nombre"] == "ea_programar_rutina").unwrap().clone();
    assert_eq!(h["confirmable"], true);
    assert_eq!(h["campos"][0]["opciones"], json!(["revision-diaria"]));
    assert_eq!(h["campos"][1]["nombre"], "calendario");

    let programar = |cal: &str| {
        json!({ "herramienta": "ea_programar_rutina", "argumentos": { "slug": "revision-diaria", "calendario": cal } })
    };
    // Simular no escribe nada y enseña las próximas ejecuciones.
    let (s, sim) = post(&f, "/api/acciones/simular", programar("*-*-* 08:00:00")).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    let texto = sim["texto"].as_str().unwrap();
    assert!(texto.contains("OnCalendar=*-*-* 08:00:00") && texto.contains("Sat 2026-10-24"), "{texto}");
    assert_eq!(sim["confirmable"], true);
    assert!(!timer.exists());

    // Calendarios malos (400) y rutinas sin registrar (409): nada escrito.
    for malo in ["daily\nExecStart=/bin/sh", "muy malo", "%h", ""] {
        let (s, _) = post(&f, "/api/acciones/simular", programar(malo)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{malo:?}");
    }
    let (s, _) = post(
        &f,
        "/api/acciones/simular",
        json!({ "herramienta": "ea_programar_rutina", "argumentos": { "slug": "no-existe", "calendario": "daily" } }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(!timer.exists());

    // El mundo cambia entre simular y guardar: la revalidación lo para.
    std::fs::create_dir_all(timer.parent().unwrap()).unwrap();
    std::fs::write(&timer, "[Timer]\nOnCalendar=daily\n").unwrap();
    let ejecutar = |sim: &Value| {
        json!({ "herramienta": sim["herramienta"], "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] })
    };
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    assert_eq!(std::fs::read_to_string(&timer).unwrap(), "[Timer]\nOnCalendar=daily\n");

    // Simulado otra vez sobre lo que hay, se guarda.
    let (_, sim) = post(&f, "/api/acciones/simular", programar("*-*-* 08:00:00")).await;
    assert!(sim["texto"].as_str().unwrap().contains("OnCalendar=daily"), "dice el horario actual: {sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(std::fs::read_to_string(&timer).unwrap().contains("\nOnCalendar=*-*-* 08:00:00\n"));
    let ordenes = std::fs::read_to_string(f._dir.path().join("ordenes.log")).unwrap();
    assert!(ordenes.contains("systemctl\n--user\nenable\nea-rutina@revision-diaria.timer\n"), "{ordenes}");
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert_eq!(rs[0]["calendario"], "*-*-* 08:00:00");

    // Desprogramar: igual, simular y ejecutar lo simulado.
    let quitar = json!({ "herramienta": "ea_desprogramar_rutina", "argumentos": { "slug": "revision-diaria" } });
    let (s, sim) = post(&f, "/api/acciones/simular", quitar).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(!timer.exists());
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert_eq!(rs[0]["calendario"], Value::Null);

    // Todo queda en el diario: el intento parado y los dos que salieron.
    let ev = f.state.store.lock().await.list_events(&ea_store::EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let de = |a: &str| ev.iter().filter(|e| e.action == a && e.detail.as_deref().unwrap_or("").contains("_rutina")).count();
    assert_eq!((de("accion_fallida"), de("accion_ejecutada")), (1, 2));
}

/// Lo que la terminal simulada de la prueba habría lanzado.
fn lanzadas(f: &Fixture) -> Vec<Vec<String>> {
    match &f.state.config.terminal {
        ea_web::servicio::terminal::Terminal::Simulada { lanzadas, .. } => lanzadas.lock().unwrap().clone(),
        _ => panic!("las pruebas usan la terminal simulada"),
    }
}

#[tokio::test]
async fn abrir_en_terminal_solo_acepta_comandos_del_catalogo() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let reversible = proponer_accion(&f, "ea_encolar_rutina", json!({ "slug": "revision-diaria" })).await;
    // Solo se borra para siempre una rutina retirada (en la papelera).
    ea_bus::retirar_rutina(&f.state.config.rutinas_dir, "revision-diaria").unwrap();
    let id = proponer_accion(&f, "ea_eliminar_rutina", json!({ "slug": "revision-diaria" })).await;

    // Texto libre, comandos, campos de más o lo que se decide desde la web: 400.
    for cuerpo in [
        json!("ea propuestas decidir 1 --aceptar"),
        json!({ "comando": "rm -rf ~" }),
        json!({ "propuesta": id, "confirmo": id }),
        json!({ "propuesta": format!("{id}; rm -rf ~") }),
        json!({ "propuesta": reversible }),
        json!({ "herramienta": "rm", "argumentos": {} }),
        json!({ "herramienta": "ea_encolar_rutina", "argumentos": { "slug": "revision-diaria" } }),
        json!({ "herramienta": "ea_eliminar_rutina", "argumentos": { "slug": "../x" } }),
        json!({ "herramienta": "ea_eliminar_rutina", "argumentos": { "slug": "x", "otro": "y" } }),
        json!({ "herramienta": "ea_eliminar_rutina", "argumentos": "slug=x" }),
    ] {
        let (s, _) = post(&f, "/api/terminal/abrir", cuerpo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    }
    let (s, _) = enviar(&f, Method::POST, "/api/terminal/abrir", "localhost:8092", None, true).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "cuerpo vacío");
    let (s, _) = post(&f, "/api/terminal/abrir", json!({ "propuesta": 9999 })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Sin la clave de sesión no se abre nada.
    let cuerpo = json!({ "propuesta": id });
    let (s, _) =
        enviar_con_clave(&f, Method::POST, "/api/terminal/abrir", "localhost:8092", Some(cuerpo), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(lanzadas(&f).is_empty(), "nada de lo anterior abre una ventana");

    // Lo que sí: el comando de la CLI como argv, sin shell ni frase.
    let (s, r) = post(&f, "/api/terminal/abrir", json!({ "propuesta": id })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["comando"], format!("ea propuestas decidir {id} --aceptar"));
    let args = json!({ "slug": "revision-diaria" });
    let (s, _) = post(&f, "/api/terminal/abrir", json!({ "herramienta": "ea_eliminar_rutina", "argumentos": args })).await;
    assert_eq!(s, StatusCode::OK);
    let ea = match &f.state.config.terminal {
        ea_web::servicio::terminal::Terminal::Simulada { ea, .. } => ea.display().to_string(),
        _ => unreachable!(),
    };
    let id_txt = id.to_string();
    let lanzador = ea_plataforma::escritorio::lanzadores_terminal()[0];
    let esperado: Vec<Vec<String>> = [
        vec![lanzador, &ea, "propuestas", "decidir", &id_txt, "--aceptar", "--esperar"],
        vec![lanzador, &ea, "acciones", "ejecutar", "ea_eliminar_rutina", "--args", r#"{"slug":"revision-diaria"}"#, "--esperar"],
    ]
    .into_iter()
    .map(|v| v.into_iter().map(String::from).collect())
    .collect();
    assert_eq!(lanzadas(&f), esperado);
    // Abrir no decide nada: la propuesta sigue pendiente y la rutina, en su sitio.
    let (_, p) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(p.as_array().unwrap().len(), 2);
    assert!(ea_bus::ruta_retirada(&f.state.config.rutinas_dir, "revision-diaria").unwrap().exists());
    let (_, d) = get(&f, "/api/diario?accion=accion_terminal").await;
    assert_eq!(d.as_array().unwrap().len(), 2);
    assert_eq!(d[0]["actor"], "usuario");
}

/// Spec 020, H4 (issue #5): el plan de `ea servicios ver` por la API (solo
/// lectura, sin `PATH` ni secretos, sin consultar al gestor en las pruebas).
#[tokio::test]
async fn el_plan_de_servicios_dice_gestor_unidades_y_binarios() {
    let f = montar();
    let (s, p) = get(&f, "/api/servicios/plan").await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert!(["systemd", "launchd", "schtasks"].contains(&p["backend"].as_str().unwrap()), "{p}");
    assert!(!p["gestor"].as_str().unwrap().is_empty());
    assert!(!p["binarios"].as_str().unwrap().is_empty());
    let nombres: Vec<&str> = p["servicios"].as_array().unwrap().iter().map(|x| x["nombre"].as_str().unwrap()).collect();
    assert_eq!(nombres, ["ea-mcp", "ea-orchestrator", "ea-web"]);
    for x in p["servicios"].as_array().unwrap() {
        assert!(x["unidad"].as_str().unwrap().contains(x["nombre"].as_str().unwrap()));
        assert!(x["binario"].as_str().unwrap().contains(x["nombre"].as_str().unwrap()));
        // Sin consultar al gestor (pruebas): ni instalado ni activo se saben.
        assert!(x["instalado"].is_null(), "{x}");
        assert_eq!(x["activo"], "desconocido");
    }
    // Cada binario que no está junto a ea-web se anuncia como «falta»; el plan lleva pasos.
    assert!(!p["pasos"].as_array().unwrap().is_empty());
    assert!(p["faltan"].is_array());
    // Nada del entorno: ni el valor de PATH ni claves.
    let texto = p.to_string();
    if let Ok(path) = std::env::var("PATH") {
        assert!(!texto.contains(&path), "el plan no debe llevar el PATH");
    }
    assert!(p["path_fijo"].is_boolean());
}

/// Spec 020, H4 (RF-018): instalar y desinstalar los servicios se abren en la
/// terminal (con `--esperar`), nunca se hacen desde la web; el resto, 400.
#[tokio::test]
async fn abrir_en_terminal_instalar_y_desinstalar_servicios() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    for cuerpo in [
        json!({ "servicios": "reiniciar" }),
        json!({ "servicios": "instalar; rm -rf ~" }),
        json!({ "servicios": 1 }),
        json!({ "servicios": "instalar", "confirmo": "s" }),
        json!({ "servicios": ["instalar"] }),
    ] {
        let (s, _) = post(&f, "/api/terminal/abrir", cuerpo.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    }
    let (s, _) = enviar_con_clave(
        &f, Method::POST, "/api/terminal/abrir", "localhost:8092", Some(json!({ "servicios": "instalar" })), true, None,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(lanzadas(&f).is_empty(), "nada de lo anterior abre una ventana");

    for (accion, comando) in [("instalar", "ea servicios instalar"), ("desinstalar", "ea servicios desinstalar")] {
        let (s, r) = post(&f, "/api/terminal/abrir", json!({ "servicios": accion })).await;
        assert_eq!(s, StatusCode::OK, "{r}");
        assert_eq!(r["comando"], comando);
    }
    let ea = match &f.state.config.terminal {
        ea_web::servicio::terminal::Terminal::Simulada { ea, .. } => ea.display().to_string(),
        _ => unreachable!(),
    };
    let lanzador = ea_plataforma::escritorio::lanzadores_terminal()[0];
    let esperado: Vec<Vec<String>> = [
        vec![lanzador, &ea, "servicios", "instalar", "--esperar"],
        vec![lanzador, &ea, "servicios", "desinstalar", "--esperar"],
    ]
    .into_iter()
    .map(|v| v.into_iter().map(String::from).collect())
    .collect();
    assert_eq!(lanzadas(&f), esperado);
    let (_, d) = get(&f, "/api/diario?accion=accion_terminal").await;
    assert_eq!(d.as_array().unwrap().len(), 2);
    assert_eq!(d[0]["actor"], "usuario");
}

/// R6 (spec 004, RF-011): retirar manda el TOML a la papelera de
/// `rutinas_dir` y desprograma el timer; la rutina retirada no se lista
/// (salvo con `?retiradas=1`) ni se lanza, y la web y la simulación de
/// `ea_encolar_rutina` por la capa de MCP (`SoloConsulta`) dan la misma
/// respuesta. Restaurar la devuelve, sin volver a programar el timer.
#[cfg(unix)]
#[tokio::test]
async fn una_rutina_retirada_no_se_lista_ni_se_lanza_y_se_restaura() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let dir = f.state.config.rutinas_dir.clone();
    let timer = f._dir.path().join("systemd-user/ea-rutina@revision-diaria.timer");
    let ejecutar = |sim: &Value| {
        json!({ "herramienta": sim["herramienta"], "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] })
    };
    let accion = |h: &str| json!({ "herramienta": h, "argumentos": { "slug": "revision-diaria" } });

    // Con horario, para ver que retirar lo quita.
    let programar =
        json!({ "herramienta": "ea_programar_rutina", "argumentos": { "slug": "revision-diaria", "calendario": "daily" } });
    let (_, sim) = post(&f, "/api/acciones/simular", programar).await;
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(timer.exists());

    // Retirar es reversible: se confirma desde la web, y la simulación dice
    // que desprograma el timer.
    let (s, sim) = post(&f, "/api/acciones/simular", accion("ea_retirar_rutina")).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    assert_eq!((sim["riesgo"].as_str(), sim["confirmable"].as_bool()), (Some("reversible"), Some(true)));
    assert!(sim["texto"].as_str().unwrap().contains("desprograma su timer (OnCalendar=daily)"), "{sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(!timer.exists(), "retirar desprograma el timer");
    assert!(!dir.join("revision-diaria.toml").exists());
    assert!(ea_bus::ruta_retirada(&dir, "revision-diaria").unwrap().exists());

    // No se lista, salvo pidiendo las retiradas; la ficha dice dónde está.
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert!(rs.as_array().unwrap().is_empty(), "{rs}");
    let (_, rs) = get(&f, "/api/rutinas?retiradas=1").await;
    assert_eq!((rs[0]["slug"].as_str(), rs[0]["ubicacion"].as_str()), (Some("revision-diaria"), Some("retirada")));
    let (_, d) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(d["ubicacion"], "retirada");

    // No se lanza: la web (409) y la simulación de la capa MCP, con el mismo texto.
    let motivo = ea_bus::Ubicacion::Retirada.motivo_no_lanzable("revision-diaria").unwrap();
    let (s, e) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(e["error"], motivo);
    let (s, e_web) = post(&f, "/api/acciones/simular", accion("ea_encolar_rutina")).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let mcp = ea_acciones::SoloConsulta { rutinas_dir: dir.clone(), boveda: f.state.boveda.clone() };
    let args = json!({ "slug": "revision-diaria" });
    let h = ea_acciones::observar(&mcp, "ea_encolar_rutina", &args).await.unwrap();
    let e_mcp =
        ea_acciones::simular_con(&*f.state.store.lock().await, &mcp, &h, "ea_encolar_rutina", &args).unwrap_err();
    assert_eq!(e_web["error"].as_str().unwrap(), e_mcp.to_string());
    assert!(e_mcp.to_string().contains(&motivo));
    let (_, ts) = get(&f, "/api/trabajos").await;
    assert!(ts.as_array().unwrap().is_empty(), "ningún trabajo creado");

    // Restaurar: vuelve a listarse y a lanzarse, sin timer.
    let (s, sim) = post(&f, "/api/acciones/simular", accion("ea_restaurar_rutina")).await;
    assert_eq!(s, StatusCode::OK, "{sim}");
    assert!(sim["texto"].as_str().unwrap().contains("No vuelve a programar su timer"), "{sim}");
    let (s, r) = post(&f, "/api/acciones/ejecutar", ejecutar(&sim)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(dir.join("revision-diaria.toml").exists() && !timer.exists());
    let (_, rs) = get(&f, "/api/rutinas").await;
    assert_eq!(rs[0]["ubicacion"], "activa");
    let (_, rs) = get(&f, "/api/rutinas?retiradas=1").await;
    assert!(rs.as_array().unwrap().is_empty());
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);

    let ev = f.state.store.lock().await.list_events(&ea_store::EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let hechas = |h: &str| {
        ev.iter().filter(|e| e.action == "accion_ejecutada" && e.detail.as_deref().unwrap_or("").contains(h)).count()
    };
    assert_eq!((hechas("ea_retirar_rutina"), hechas("ea_restaurar_rutina")), (1, 1));
}

// ------------------------------------------------------------ biblioteca (018)

fn ids(v: &Value) -> Vec<String> {
    v["elementos"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect()
}

fn sin_secretos(t: &str) {
    for s in SECRETOS {
        assert!(!t.contains(s), "la respuesta lleva el secreto {s}: {t}");
    }
}

#[tokio::test]
async fn la_biblioteca_lista_y_filtra() {
    let f = montar();
    let (st, v) = get(&f, "/api/biblioteca").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let todos = ids(&v);
    for id in [SKILL, MCP_CLAUDE, MCP_CODEX, MCP_CODEX_APAGADO, "opencode/mcp/usuario/remoto-oc"] {
        assert!(todos.iter().any(|x| x == id), "falta {id}: {todos:?}");
    }
    assert_eq!(v["total"].as_u64().unwrap() as usize, todos.len());
    assert!(v["categorias"].as_array().unwrap().iter().any(|c| c == "usuario"), "{v}");
    assert!(v["avisos"].is_array());
    assert!(v["agentes_sesion"].as_array().unwrap().iter().any(|a| a == "claude-code"));

    let (_, v) = get(&f, "/api/biblioteca?agente=codex").await;
    assert!(ids(&v).iter().all(|i| i.starts_with("codex/")) && !ids(&v).is_empty(), "{v}");
    let (_, v) = get(&f, "/api/biblioteca?agente=claude-code&tipo=skill").await;
    assert!(ids(&v).iter().all(|i| i.starts_with("claude-code/skill/")), "{v}");
    assert!(ids(&v).contains(&SKILL.to_string()));
    let (_, v) = get(&f, "/api/biblioteca?agente=codex&activado=no").await;
    assert_eq!(ids(&v), vec![MCP_CODEX_APAGADO.to_string()], "{v}");
    let (_, v) = get(&f, "/api/biblioteca?categoria=usuario").await;
    assert!(ids(&v).contains(&SKILL.to_string()) && ids(&v).iter().all(|i| todos.contains(i)), "{v}");
    assert!(v["elementos"].as_array().unwrap().iter().all(|e| e["categoria"] == "usuario"), "{v}");
    // Las categorías son las de todo el inventario, no las de lo filtrado.
    let (_, v) = get(&f, "/api/biblioteca?categoria=otra").await;
    assert!(v["elementos"].as_array().unwrap().is_empty());
    assert!(!v["categorias"].as_array().unwrap().is_empty());
    for mala in ["tipo=nada", "activado=quizas"] {
        let (st, _) = get(&f, &format!("/api/biblioteca?{mala}")).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{mala}");
    }
}

#[tokio::test]
async fn la_ficha_trae_el_elemento_y_su_uso() {
    let f = montar();
    // El id lleva «/»: la página lo codifica, pero sin codificar también vale.
    for ruta in [
        "/api/biblioteca/claude-code%2Fskill%2Fusuario%2Fpropia".to_string(),
        format!("/api/biblioteca/{SKILL}"),
    ] {
        let (st, v) = get(&f, &ruta).await;
        assert_eq!(st, StatusCode::OK, "{ruta}: {v}");
        assert_eq!(v["id"], SKILL);
        assert_eq!(v["mecanismo"], "mover");
        assert_eq!(v["activado"], "si");
        assert_eq!(v["cambio"]["posible"], true);
        assert_eq!(v["uso"]["total"], 4, "{v}");
        assert_eq!(v["uso"]["por"][0]["actor"], "claude-code");
        assert!(v["sesion"].as_array().unwrap().iter().any(|a| a == "claude-code"));
    }
    // Un MCP de Claude Code solo se acota en sesión: lo dice, sin botón de activar.
    let (_, v) = get(&f, "/api/biblioteca/claude-code%2Fmcp%2Fusuario%2Fremoto-claude").await;
    assert_eq!(v["cambio"]["posible"], false);
    assert!(v["cambio"]["motivo"].as_str().unwrap().contains("sesión"), "{v}");
    assert_eq!(v["def_mcp"]["transporte"], "http");
    assert!(v["uso"].is_null(), "sin uso: {v}");
    let (st, _) = get(&f, "/api/biblioteca/claude-code%2Fskill%2Fusuario%2Fno-existe").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn el_uso_es_un_informe_con_cache() {
    let f = montar();
    let (st, v) = get(&f, "/api/biblioteca/uso").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let u = v["usos"].as_array().unwrap().iter().find(|u| u["id"] == SKILL).expect("uso de la skill");
    assert_eq!(u["total"], 4);
    assert!(u["ultimo"].is_string());
    assert!(v["actividad"].is_array() && v["avisos"].is_array() && v["estadisticas"].is_object());
    // La segunda vez sale de la memoria: mismo informe.
    let (_, v2) = get(&f, "/api/biblioteca/uso").await;
    assert_eq!(v, v2);
}

#[tokio::test]
async fn ninguna_respuesta_de_la_biblioteca_lleva_secretos() {
    let f = montar();
    let mut todo = String::new();
    for ruta in [
        "/api/biblioteca".to_string(),
        "/api/biblioteca/uso".to_string(),
        "/api/biblioteca/claude-code%2Fmcp%2Fusuario%2Fremoto-claude".to_string(),
        "/api/biblioteca/claude-code%2Fmcp%2Fusuario%2Fstdio-claude".to_string(),
        "/api/biblioteca/codex%2Fmcp%2Fusuario%2Fdocs".to_string(),
        "/api/biblioteca/opencode%2Fmcp%2Fusuario%2Fremoto-oc".to_string(),
    ] {
        let (st, t) = enviar(&f, Method::GET, &ruta, "localhost:8092", None, false).await;
        assert_eq!(st, StatusCode::OK, "{ruta}");
        todo += &t;
    }
    // Sí salen los nombres de las variables y cabeceras, nunca sus valores.
    assert!(todo.contains("API_KEY") && todo.contains("Authorization"), "se esperaban los nombres");
    // Y las simulaciones de sesión (con el comando) tampoco.
    for (agente, hs) in [
        ("claude-code", json!([MCP_CLAUDE, "claude-code/mcp/usuario/stdio-claude", SKILL])),
        ("codex", json!([MCP_CODEX])),
        ("opencode", json!(["opencode/mcp/usuario/remoto-oc"])),
    ] {
        let (_, t) = enviar(
            &f,
            Method::POST,
            "/api/biblioteca/simular",
            "localhost:8092",
            Some(json!({ "herramienta": "ea_lanzar_sesion", "argumentos": { "agente": agente, "herramientas": hs } })),
            true,
        )
        .await;
        todo += &t;
    }
    sin_secretos(&todo);
}

#[tokio::test]
async fn desactivar_y_activar_pasa_por_simulacion_y_se_revalida() {
    let f = montar();
    let skill = f._dir.path().join("home/.claude/skills/propia");
    let guardada = f._dir.path().join("estado-biblioteca/biblioteca/desactivadas/claude-code/skill/propia");
    assert!(skill.is_dir());

    let (st, sim) = post(
        &f,
        "/api/biblioteca/simular",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": { "elemento": SKILL } }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{sim}");
    assert_eq!(sim["riesgo"], "reversible");
    assert_eq!(sim["confirmable"], true);
    assert!(sim["texto"].as_str().unwrap().contains("moviendo"), "{sim}");
    assert!(skill.is_dir(), "simular no toca nada");

    // Si el mundo cambia entre simular y ejecutar, no se hace nada.
    let (st, e) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": sim["argumentos"], "precondicion": { "x": 1 } }),
    )
    .await;
    assert!(skill.is_dir(), "{st} {e}");

    let (st, r) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert!(!skill.exists(), "la skill se movió");
    assert!(guardada.exists(), "y está en el almacén del ecosistema");
    assert!(r["evento"].is_number(), "queda en el diario: {r}");

    // La biblioteca lo refleja: la skill guardada sigue en el inventario, desactivada.
    let (_, v) = get(&f, "/api/biblioteca/claude-code%2Fskill%2Fusuario%2Fpropia").await;
    assert_eq!(v["activado"], "no", "{v}");

    // Activar lo deshace.
    let (_, sim) = post(
        &f,
        "/api/biblioteca/simular",
        json!({ "herramienta": "ea_activar_herramienta", "argumentos": { "elemento": SKILL } }),
    )
    .await;
    let (st, r) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_activar_herramienta", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert!(skill.is_dir() && !guardada.exists());
}

#[tokio::test]
async fn lo_que_no_se_puede_cambiar_se_rechaza() {
    let f = montar();
    // Un MCP de Claude Code (~/.claude.json) solo se acota en sesión.
    let (st, e) = post(
        &f,
        "/api/biblioteca/simular",
        json!({ "herramienta": "ea_desactivar_herramienta", "argumentos": { "elemento": MCP_CLAUDE } }),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{e}");
    // Solo las tres herramientas de la sección; ni otras ni un directorio a pedido.
    for cuerpo in [
        json!({ "herramienta": "ea_reiniciar_servicio", "argumentos": { "servicio": "ea-mcp" } }),
        json!({ "herramienta": "ea_lanzar_sesion", "argumentos": { "agente": "codex", "herramientas": [MCP_CODEX], "directorio": tmp!() } }),
    ] {
        let (st, _) = post(&f, "/api/biblioteca/simular", cuerpo).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn proponer_y_aceptar_una_sesion_abre_la_terminal_sin_secretos() {
    let f = montar();
    let args = json!({ "agente": "claude-code", "herramientas": [MCP_CLAUDE, SKILL] });
    let (st, sim) =
        post(&f, "/api/biblioteca/simular", json!({ "herramienta": "ea_lanzar_sesion", "argumentos": args })).await;
    assert_eq!(st, StatusCode::OK, "{sim}");
    let texto = sim["texto"].as_str().unwrap();
    assert!(texto.contains("claude") && texto.contains("--strict-mcp-config"), "{sim}");
    sin_secretos(&sim.to_string());
    assert!(f.terminales.lock().unwrap().is_empty(), "simular no abre nada");

    let (st, r) = post(
        &f,
        "/api/biblioteca/ejecutar",
        json!({ "herramienta": "ea_lanzar_sesion", "argumentos": sim["argumentos"], "precondicion": sim["precondicion"] }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    sin_secretos(&r.to_string());
    let abiertas = f.terminales.lock().unwrap().clone();
    assert_eq!(abiertas.len(), 1, "una terminal");
    assert!(abiertas[0].iter().any(|a| a == "claude"), "{abiertas:?}");
    sin_secretos(&format!("{abiertas:?}"));
    // Los secretos solo están en los ficheros efímeros, con permisos 0600.
    let mcp = std::fs::read_dir(f._dir.path().join("runtime/ea-biblioteca/sesiones"))
        .unwrap()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten())
        .map(|e| e.path())
        .find(|p| std::fs::read_to_string(p).is_ok_and(|t| t.contains("SECRETO_HEADER_CLAUDE")))
        .expect("el fichero efímero con la definición completa");
    if let Some(m) = ea_plataforma::fs::permisos_unix(&std::fs::metadata(mcp).unwrap()) {
        assert_eq!(m & 0o777, 0o600);
    }
}

#[tokio::test]
async fn la_categoria_propia_va_por_ajustes() {
    let f = montar();
    let clave = |aj: &Value| {
        aj["secciones"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["claves"].as_array().unwrap().iter())
            .find(|c| c["clave"] == "biblioteca.categorias")
            .expect("la clave está en la vista de ajustes")
            .clone()
    };
    let (_, aj) = get(&f, "/api/ajustes").await;
    let c = clave(&aj);
    assert_eq!(c["tipo"], "mapa", "{c}");
    assert_eq!(c["edicion"], "web");
    let (st, r) = post(
        &f,
        "/api/ajustes",
        json!({ "version": aj["version"], "cambios": { "biblioteca.categorias": { SKILL: "trabajo" } } }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, v) = get(&f, "/api/biblioteca?categoria=trabajo").await;
    assert_eq!(ids(&v), vec![SKILL.to_string()], "{v}");
    assert!(v["categorias"].as_array().unwrap().iter().any(|c| c == "trabajo"));
    let (_, aj) = get(&f, "/api/ajustes").await;
    assert_eq!(clave(&aj)["valor"][SKILL], "trabajo");
}

// ------------------------------------------------------------------ equipos

/// Binarios de mentira para los cinco agentes del catálogo y unos ajustes que
/// los usan, con `orquestador.paralelo = paralelo`: así la prueba no depende
/// del PATH real. Devuelve el repo git donde se despiertan los equipos.
fn preparar_equipos(f: &Fixture, paralelo: u32) -> std::path::PathBuf {
    let bins = f._dir.path().join("bins");
    std::fs::create_dir_all(&bins).unwrap();
    let mut toml = format!("[orquestador]\nparalelo = {paralelo}\n");
    for id in ea_ajustes::IDS_AGENTES {
        // Un binario falso que `buscar_binario` da por ejecutable en cada sistema.
        let p = bins.join(if cfg!(windows) { format!("{id}.cmd") } else { id.to_string() });
        std::fs::write(&p, if cfg!(windows) { "@echo off\r\n" } else { "#!/bin/sh\n" }).unwrap();
        #[cfg(unix)]
        ea_plataforma::fs::poner_permisos_unix(&p, 0o755).unwrap();
        toml += &format!("[agentes.{id}]\nbin = \"{}\"\n", p.display().to_string().replace('\\', "/"));
    }
    let fichero = f.state.config.ajustes.fichero();
    std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
    std::fs::write(&fichero, toml).unwrap();
    repo_git(f)
}

fn miembros(lista: &[(&str, &str)]) -> Value {
    Value::Array(lista.iter().map(|(a, r)| json!({ "agente": a, "rol": r })).collect())
}

fn despertar_cuerpo(wt: &std::path::Path, lista: &[(&str, &str)], terminal: bool) -> Value {
    json!({ "titulo": "Equipo de prueba", "objetivo": "Probar los equipos", "worktree": wt, "miembros": miembros(lista), "terminal": terminal })
}

const BASICO: [(&str, &str); 2] = [("claude-code", "planificador"), ("qwen-code", "ejecutor")];

#[tokio::test]
async fn los_equipos_se_listan_vacios_y_con_uno() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let (s, v) = get(&f, "/api/equipos").await;
    assert_eq!((s, v), (StatusCode::OK, json!([])));

    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &BASICO, false)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    assert_eq!(r["equipo"]["estado"], "despierto");
    assert_eq!(r["equipo"]["creado_por"], "usuario");
    // Sin terminal pedida: solo dice cómo abrir la sesión del planificador.
    assert_eq!(r["sesiones"][0]["agente"], "claude-code");
    assert_eq!(r["sesiones"][0]["abierta"], false);
    assert_eq!(r["sesiones"][0]["comando"], format!("ea equipo sesion {id} claude-code"));
    assert!(lanzadas(&f).is_empty());

    let (_, v) = get(&f, "/api/equipos").await;
    assert_eq!(v.as_array().unwrap().len(), 1, "{v}");
    assert_eq!(v[0]["id"], id.as_str());
    assert_eq!(v[0]["estado"], "despierto");
    assert_eq!(v[0]["trabajos"], 0);
    assert_eq!(v[0]["miembros"], json!([
        { "agente": "claude-code", "rol": "planificador", "viva": false },
        { "agente": "qwen-code", "rol": "ejecutor", "viva": false },
    ]));

    let (s, v) = get(&f, &format!("/api/equipos/{id}")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["titulo"], "Equipo de prueba");
    assert_eq!(v["miembros"][1]["pendientes"], 0);
    assert_eq!(v["trabajos_total"], 0);

    assert_eq!(get(&f, "/api/equipos/no-existe").await.0, StatusCode::NOT_FOUND);
    assert_eq!(get(&f, "/api/equipos/a.b").await.0, StatusCode::BAD_REQUEST);
    // El diario lo cuenta con actor usuario.
    let (_, d) = get(&f, "/api/diario?accion=equipo_").await;
    assert!(d.to_string().contains("equipo_despertado"), "{d}");
}

#[tokio::test]
async fn despertar_y_dormir_exigen_la_clave() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let cuerpo = despertar_cuerpo(&wt, &BASICO, false);
    for clave in [None, Some("mala")] {
        let (s, t) = enviar_con_clave(&f, Method::POST, "/api/equipos", "localhost:8092", Some(cuerpo.clone()), true, clave).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{t}");
    }
    assert_eq!(get(&f, "/api/equipos").await.1, json!([]), "sin clave no se despierta nada");
    let (_, r) = post(&f, "/api/equipos", cuerpo).await;
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    let (s, _) = enviar_con_clave(&f, Method::POST, &format!("/api/equipos/{id}/dormir"), "localhost:8092", Some(json!({})), true, None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(get(&f, &format!("/api/equipos/{id}")).await.1["estado"], "despierto");
}

#[tokio::test]
async fn despertar_con_terminal_abre_una_por_planificador_y_revisor() {
    let f = montar();
    let wt = preparar_equipos(&f, 2);
    let lista = [("claude-code", "planificador"), ("antigravity", "revisor"), ("qwen-code", "ejecutor")];
    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &lista, true)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["equipo"]["id"].as_str().unwrap();
    assert_eq!(r["sesiones"].as_array().unwrap().len(), 2, "{r}");
    assert!(r["sesiones"].as_array().unwrap().iter().all(|x| x["abierta"] == true && x["error"].is_null()));
    // Solo planificador y revisor; argv cerrado `ea equipo sesion <id> <agente>`.
    let ea = f._dir.path().join("bin/ea").display().to_string();
    let esperado = |agente: &str| vec![ea_plataforma::escritorio::lanzadores_terminal()[0].to_string(), ea.clone(), "equipo".into(), "sesion".into(), id.into(), agente.into()];
    assert_eq!(lanzadas(&f), vec![esperado("claude-code"), esperado("antigravity")]);
    let (_, d) = get(&f, "/api/diario?accion=equipo_sesion").await;
    assert_eq!(d.as_array().unwrap().len(), 2, "{d}");
    assert!(d.to_string().contains("equipo_sesion_abierta"));
}

#[tokio::test]
async fn despertar_a_todos_reparte_segun_lo_que_admite_cada_agente() {
    let f = montar();
    let wt = preparar_equipos(&f, 3);
    let (s, r) = post(&f, "/api/equipos", json!({ "titulo": "Todos", "objetivo": "Todos juntos", "worktree": wt, "todos": true })).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let m = r["equipo"]["miembros"].as_array().unwrap();
    let rol = |a: &str| m.iter().find(|x| x["agente"] == a).unwrap()["rol"].clone();
    assert_eq!(m.len(), 5);
    assert_eq!((rol("claude-code"), rol("antigravity")), (json!("planificador"), json!("planificador")));
    assert_eq!((rol("qwen-code"), rol("codex"), rol("opencode")), (json!("ejecutor"), json!("ejecutor"), json!("ejecutor")));
    assert_eq!(r["omitidos"], json!([]));
}

#[tokio::test]
async fn despertar_rechaza_lo_que_la_cli_rechaza() {
    let f = montar();
    let wt = preparar_equipos(&f, 1);
    let rechaza = |cuerpo: Value, texto: &'static str| {
        let f = &f;
        async move {
            let (s, r) = post(f, "/api/equipos", cuerpo).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
            let e = r["error"].as_str().unwrap_or_default().to_string();
            assert!(e.contains(texto), "{texto:?} en {e:?}");
        }
    };
    // agente repetido
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador"), ("claude-code", "revisor"), ("qwen-code", "ejecutor")], false), "más de una vez").await;
    // rol que el agente no admite, rol inventado y agente fuera del catálogo
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador"), ("antigravity", "ejecutor")], false), "no puede ser ejecutor").await;
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "jefe"), ("qwen-code", "ejecutor")], false), "rol de equipo").await;
    rechaza(despertar_cuerpo(&wt, &[("nadie", "planificador"), ("qwen-code", "ejecutor")], false), "catálogo").await;
    // sin ejecutor, o sin miembros, o con las dos formas a la vez
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador")], false), "ejecutor").await;
    rechaza(despertar_cuerpo(&wt, &[], false), "todos").await;
    let mut ambos = despertar_cuerpo(&wt, &BASICO, false);
    ambos["todos"] = json!(true);
    rechaza(ambos, "no las dos cosas").await;
    // tope orquestador.paralelo = 1: dos ejecutores no caben
    rechaza(despertar_cuerpo(&wt, &[("claude-code", "planificador"), ("qwen-code", "ejecutor"), ("codex", "ejecutor")], false), "orquestador.paralelo").await;
    // campos sueltos, título vacío y worktree que no existe
    let mut extra = despertar_cuerpo(&wt, &BASICO, false);
    extra["creado_por"] = json!("claude-code");
    rechaza(extra, "creado_por").await;
    let mut sin_titulo = despertar_cuerpo(&wt, &BASICO, false);
    sin_titulo["titulo"] = json!("  ");
    rechaza(sin_titulo, "titulo").await;
    let mut sin_wt = despertar_cuerpo(&wt, &BASICO, false);
    sin_wt["worktree"] = json!(concat!(tmp!(), "/no/existe/de/verdad"));
    rechaza(sin_wt, "no existe").await;
    // un binario que falta
    let fichero = f.state.config.ajustes.fichero();
    let t = std::fs::read_to_string(&fichero).unwrap();
    std::fs::write(&fichero, t.replace("bins/qwen-code", "bins/no-esta")).unwrap();
    rechaza(despertar_cuerpo(&wt, &BASICO, false), "no está disponible").await;
    std::fs::write(&fichero, t).unwrap();

    assert_eq!(get(&f, "/api/equipos").await.1, json!([]), "nada se ha escrito");
    assert!(lanzadas(&f).is_empty());

    // Un agente solo está en un equipo despierto; dormirlo lo libera.
    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &BASICO, false)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    rechaza(despertar_cuerpo(&wt, &[("antigravity", "planificador"), ("qwen-code", "ejecutor")], false), "ya está en el equipo despierto").await;
    let (s, d) = post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["estado"], "dormido");
    assert_eq!(get(&f, "/api/equipos").await.1[0]["estado"], "dormido");
    let (s, _) = post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    assert_eq!(s, StatusCode::OK, "dormir uno dormido no hace nada");
    assert_eq!(post(&f, "/api/equipos/no-existe/dormir", json!({})).await.0, StatusCode::NOT_FOUND);
    let (s, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &[("antigravity", "planificador"), ("qwen-code", "ejecutor")], false)).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
}

#[tokio::test]
async fn dormir_devuelve_lo_pendiente_a_la_cola_del_ejecutor() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = preparar_equipos(&f, 2);
    let (_, r) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &BASICO, false)).await;
    let id = r["equipo"]["id"].as_str().unwrap().to_string();
    let (s, t) = post(&f, "/api/trabajos", json!({ "titulo": "Asignado", "worktree": wt, "objetivo": "x", "criterio": "true",
        "equipo": id, "asignado_a": "qwen-code" })).await;
    assert_eq!(s, StatusCode::CREATED, "{t}");
    let (_, v) = get(&f, &format!("/api/equipos/{id}")).await;
    assert_eq!(v["miembros"][1]["pendientes"], 1, "{v}");
    assert_eq!(v["trabajos"][0]["asignado_a"], "qwen-code");
    assert_eq!(get(&f, "/api/equipos").await.1[0]["trabajos"], 1);
    let (_, colas) = get(&f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().unwrap().len(), 0, "{colas}");
    post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    let (_, colas) = get(&f, "/api/colas").await;
    assert_eq!(colas["modelo-local"].as_array().unwrap().len(), 1, "{colas}");
    let (_, v) = get(&f, &format!("/api/equipos/{id}")).await;
    assert_eq!(v["miembros"][1]["pendientes"], 0);
}

#[tokio::test]
async fn los_agentes_traen_su_presencia() {
    let f = montar();
    let fichero = f.state.config.ajustes.fichero();
    std::fs::create_dir_all(fichero.parent().unwrap()).unwrap();
    std::fs::write(&fichero, "[modelos.local]\nbase_url = \"http://127.0.0.1:9/v1\"\n").unwrap();
    let ag = |v: &Value, id: &str| v["agentes"].as_array().unwrap().iter().find(|a| a["id"] == id).unwrap().clone();
    let (_, v) = get(&f, "/api/agentes").await;
    assert!(ag(&v, "qwen-code")["presencia"].is_null(), "{v}");

    let bus = ea_bus::Bus::abrir_en(&f.state.config.estado_dir).unwrap();
    // Viva: el pid de esta prueba existe y el latido es de ahora.
    let mut p = ea_bus::Presencia::nueva("qwen-code", ea_bus::RolEquipo::Ejecutor, Some("e1"), std::process::id());
    p.trabajo = Some("20261021-101500-ab12".into());
    bus.anotar_presencia(&p).unwrap();
    // Caducada: ningún proceso tiene el pid 0.
    bus.anotar_presencia(&ea_bus::Presencia::nueva("claude-code", ea_bus::RolEquipo::Planificador, None, 0)).unwrap();
    let (_, v) = get(&f, "/api/agentes").await;
    let q = ag(&v, "qwen-code");
    assert_eq!(q["presencia"]["viva"], true, "{q}");
    assert_eq!(q["presencia"]["equipo"], "e1");
    assert_eq!(q["presencia"]["trabajo"], "20261021-101500-ab12");
    assert_eq!(q["presencia"]["rol"], "ejecutor");
    assert_eq!(ag(&v, "claude-code")["presencia"]["viva"], false);
    assert!(ag(&v, "codex")["presencia"].is_null());
    // El catálogo sigue siendo el de siempre.
    assert_eq!(q["rol"], "ejecutor");
}

#[tokio::test]
async fn crear_un_trabajo_para_un_equipo_valida_como_el_bus() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    let wt = preparar_equipos(&f, 2);
    let cuerpo = |extra: Value| {
        let mut m = mision(wt.to_str().unwrap());
        for (k, v) in extra.as_object().unwrap() {
            m[k] = v.clone();
        }
        m
    };
    // Sin equipo, asignado y revisor no valen.
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "asignado_a": "qwen-code" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert!(r["error"].as_str().unwrap().contains("necesitan un equipo"), "{r}");
    let (s, _) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": "no-existe" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let lista = [("claude-code", "planificador"), ("antigravity", "revisor"), ("qwen-code", "ejecutor"), ("codex", "ejecutor")];
    let (_, e) = post(&f, "/api/equipos", despertar_cuerpo(&wt, &lista, false)).await;
    let id = e["equipo"]["id"].as_str().unwrap().to_string();
    // El asignado tiene que ser un ejecutor del equipo, y el revisor, no el mismo.
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "asignado_a": "opencode" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "asignado_a": "claude-code" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "revisor": "qwen-code" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    // Los selectores del formulario mandan "" cuando no se elige nada.
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": "", "asignado_a": "", "revisor": " " }))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");

    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id, "asignado_a": "codex", "revisor": "antigravity", "worktree_propio": true }))).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    let t = r["id"].as_str().unwrap().to_string();
    let (_, d) = get(&f, &format!("/api/trabajos/{t}")).await;
    assert_eq!(d["orden"]["equipo"], id.as_str());
    assert_eq!(d["orden"]["asignado_a"], "codex");
    assert_eq!(d["orden"]["revisor"], "antigravity");
    assert_eq!(d["orden"]["worktree_propio"], true);
    // Va a la cola de su ejecutor, no a la del rol, y el equipo lo anota.
    let (_, v) = get(&f, &format!("/api/equipos/{id}")).await;
    let codex = v["miembros"].as_array().unwrap().iter().find(|m| m["agente"] == "codex").unwrap().clone();
    assert_eq!(codex["cola"], json!([t.as_str()]));
    assert_eq!(v["trabajos"][0]["id"], t.as_str());
    assert_eq!(v["trabajos"][0]["revisor"], "antigravity");
    // Un equipo dormido no admite trabajo.
    post(&f, &format!("/api/equipos/{id}/dormir"), json!({})).await;
    let (s, r) = post(&f, "/api/trabajos", cuerpo(json!({ "equipo": id }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert!(r["error"].as_str().unwrap().contains("dormido"), "{r}");
}
