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

const RUTINA_BUS: &str = "[rutina]\nslug = \"revision-diaria\"\nversion = 1\nworktree = \"/tmp\"\ncriterio_exito = \"true\"\n[[pasos]]\ntexto = \"revisa\"\n";

/// Las pruebas que lanzan rutinas comparten la variable de entorno del
/// directorio de estado del bus: una a una, cada cual con el suyo.
static BUS_EXCLUSIVO: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn aislar_bus(f: &Fixture) -> tokio::sync::MutexGuard<'static, ()> {
    let g = BUS_EXCLUSIVO.lock().await;
    unsafe { std::env::set_var("EA_ESTADO_DIR", f._dir.path().join("estado")) };
    g
}

fn montar() -> Fixture {
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
    };
    let state = Arc::new(AppState::new(store, config).expect("estado"));
    Fixture { app: api::router(state.clone()), state, _dir: dir }
}

async fn enviar(
    f: &Fixture,
    metodo: Method,
    ruta: &str,
    host: &str,
    cuerpo: Option<Value>,
    json_ct: bool,
) -> (StatusCode, String) {
    let mut req = Request::builder().method(metodo).uri(ruta).header("host", host);
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
        "/api/tareas",
        "localhost:8092",
        Some(json!({ "titulo": "x", "asignado_a": "claude-code", "solicita": "y" })),
        false,
    )
    .await;
    assert_eq!(s, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn ciclo_de_vida_de_una_tarea() {
    let f = montar();

    let (s, t) = post(
        &f,
        "/api/tareas",
        json!({ "titulo": " Probar web ", "asignado_a": "claude-code", "solicita": "Ver que funciona" }),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(t["titulo"], "Probar web");
    assert_eq!(t["origen"], "boton");
    assert_eq!(t["creado_por"], "usuario");
    assert_eq!(t["estado"], "pendiente");
    let id = t["id"].as_i64().unwrap();

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["tareas"]["pendiente"], 1);

    let (s, t) = post(&f, &format!("/api/tareas/{id}/estado"), json!({ "estado": "en_progreso" })).await;
    assert_eq!((s, t["estado"].as_str()), (StatusCode::OK, Some("en_progreso")));
    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["agentes"][0]["nombre"], "claude-code");
    assert_eq!(e["agentes"][0]["tarea"]["id"], id);

    let (s, t) = post(
        &f,
        &format!("/api/tareas/{id}/estado"),
        json!({ "estado": "completada", "resultado": "hecho" }),
    )
    .await;
    assert_eq!((s, t["resultado"].as_str()), (StatusCode::OK, Some("hecho")));

    let (s, ficha) = get(&f, &format!("/api/tareas/{id}")).await;
    assert_eq!(s, StatusCode::OK);
    let acciones: Vec<_> = ficha["eventos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["accion"].as_str().unwrap())
        .filter(|a| *a != "accion_ejecutada") // el registro de la capa interna, una por cambio
        .collect();
    assert_eq!(acciones, ["tarea_creada", "tarea_iniciada", "tarea_completada"]);
    let (_, d) = get(&f, "/api/diario").await;
    let n = d.as_array().unwrap().iter().filter(|e| e["accion"] == "accion_ejecutada").count();
    assert_eq!(n, 2, "cada cambio de estado pasa por la capa interna y queda registrado");

    let (_, lista) = get(&f, "/api/tareas?estado=completada").await;
    assert_eq!(lista.as_array().unwrap().len(), 1);
    let (_, lista) = get(&f, "/api/tareas?estado=pendiente").await;
    assert!(lista.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn valida_las_entradas() {
    let f = montar();

    let (s, _) = post(&f, "/api/tareas", json!({ "titulo": "  ", "asignado_a": "a", "solicita": "b" })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = get(&f, "/api/tareas?estado=inventado").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = get(&f, "/api/tareas/999").await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _) = post(&f, "/api/tareas/999/estado", json!({ "estado": "completada" })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _) = get(&f, "/api/diario?desde=ayer").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn el_diario_oculta_los_latidos_salvo_que_se_pidan() {
    let f = montar();
    {
        let store = f.state.store.lock().await;
        for accion in ["heartbeat", "heartbeat", "tarea_creada"] {
            store
                .append_event(NewEvent {
                    actor: "orquestador".into(),
                    action: accion.into(),
                    task_id: None,
                    outcome: Some(EventOutcome::Ok),
                    duration_ms: None,
                    detail: None,
                })
                .unwrap();
        }
    }

    let (_, sin) = get(&f, "/api/diario").await;
    assert_eq!(sin.as_array().unwrap().len(), 1);
    assert_eq!(sin[0]["accion"], "tarea_creada");

    let (_, con) = get(&f, "/api/diario?latidos=1").await;
    assert_eq!(con.as_array().unwrap().len(), 3);

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["orquestador"]["vivo"], true);
}

#[tokio::test]
async fn decidir_una_propuesta() {
    let f = montar();
    let id = {
        let store = f.state.store.lock().await;
        let run = store.start_routine_run("revision-diaria", None).unwrap();
        store.create_propuesta(run.id, "Revisar incidencias").unwrap().id
    };

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["propuestas_pendientes"], 1);

    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista[0]["rutina"], "revision-diaria");

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

    let (s, t) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(t["rutina"], "revision-diaria");
    let id = t["trabajo"].as_str().unwrap().to_string();
    let (_, j) = get(&f, &format!("/api/trabajos/{id}")).await;
    assert_eq!(j["estado"]["estado"], "en_cola");
    let (s, _) = post(&f, "/api/rutinas/revision-diaria/lanzar", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "ya hay una ejecución en curso");

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
    for ruta in ["/", "/app.css", "/app.js"] {
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
    ea_acciones::proponer(&store, &sis, &hechos, "claude-code", None, None, herramienta, &args)
        .unwrap()
        .0
        .id
}

#[tokio::test]
async fn aceptar_una_propuesta_con_accion_la_ejecuta_una_sola_vez() {
    let f = montar();
    let (_, t) = post(&f, "/api/tareas", json!({ "titulo": "x", "asignado_a": "claude-code", "solicita": "y" })).await;
    let tid = t["id"].as_i64().unwrap();
    let id = proponer_accion(&f, "ea_cambiar_estado_tarea", json!({ "id": tid, "estado": "completada" })).await;

    let (_, lista) = get(&f, "/api/propuestas?estado=pendiente").await;
    assert_eq!(lista[0]["accion"]["herramienta"], "ea_cambiar_estado_tarea");
    assert_eq!(lista[0]["accion"]["riesgo"], "reversible");
    assert_eq!(lista[0]["proponente"], "claude-code");
    assert!(lista[0]["accion"]["simulacion"].as_str().unwrap().contains("completada"));
    let (_, t) = get(&f, &format!("/api/tareas/{tid}")).await;
    assert_eq!(t["tarea"]["estado"], "pendiente", "proponer no ejecuta");

    let ruta = format!("/api/propuestas/{id}/decidir");
    let (s, p) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["estado"], "aceptada");
    assert_eq!(p["ejecutada"]["estado"], "ejecutada");
    let (_, t) = get(&f, &format!("/api/tareas/{tid}")).await;
    assert_eq!(t["tarea"]["estado"], "completada");

    let (s, _) = post(&f, &ruta, json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, d) = get(&f, "/api/diario").await;
    let ejecuciones = d.as_array().unwrap().iter().filter(|e| e["accion"] == "accion_ejecutada").count();
    assert_eq!(ejecuciones, 1);
}

#[tokio::test]
async fn una_propuesta_cuyo_estado_cambio_falla_con_mensaje_claro() {
    let f = montar();
    let (_, t) = post(&f, "/api/tareas", json!({ "titulo": "x", "asignado_a": "claude-code", "solicita": "y" })).await;
    let tid = t["id"].as_i64().unwrap();
    let id = proponer_accion(&f, "ea_cambiar_estado_tarea", json!({ "id": tid, "estado": "completada" })).await;
    post(&f, &format!("/api/tareas/{tid}/estado"), json!({ "estado": "en_progreso" })).await;

    let (s, p) = post(&f, &format!("/api/propuestas/{id}/decidir"), json!({ "decision": "aceptar" })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["ejecutada"]["estado"], "fallida");
    assert!(p["ejecutada"]["detalle"].as_str().unwrap().contains("cambió desde la simulación"));
    let (_, t) = get(&f, &format!("/api/tareas/{tid}")).await;
    assert_eq!(t["tarea"]["estado"], "en_progreso");
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
        ea_acciones::proponer(&store, &c, &Default::default(), "claude-code", None, None, "ea_promover_nota", &args)
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

    for ruta in ["/api/estado", "/api/tareas", "/api/diario", "/api/propuestas"] {
        let (s, _) = tokio::time::timeout(std::time::Duration::from_secs(5), get(&f, ruta))
            .await
            .unwrap_or_else(|_| panic!("{ruta} se quedó esperando a la acción en curso"));
        assert_eq!(s, StatusCode::OK, "{ruta}");
    }
}

#[tokio::test]
async fn la_portada_reune_lo_pendiente_lo_activo_y_lo_reciente() {
    let f = montar();

    let (s, i) = get(&f, "/api/inicio").await;
    assert_eq!(s, StatusCode::OK);
    assert!(i["propuestas"].as_array().unwrap().is_empty());
    assert!(i["activas"].as_array().unwrap().is_empty());
    assert!(i["recientes"].as_array().unwrap().is_empty());

    let mut ids = Vec::new();
    for titulo in ["una", "otra"] {
        let (_, t) = post(
            &f,
            "/api/tareas",
            json!({ "titulo": titulo, "asignado_a": "claude-code", "solicita": "x" }),
        )
        .await;
        ids.push(t["id"].as_i64().unwrap());
    }
    post(&f, &format!("/api/tareas/{}/estado", ids[0]), json!({ "estado": "cancelada" })).await;
    {
        let store = f.state.store.lock().await;
        let run = store.start_routine_run("revision-diaria", None).unwrap();
        store.create_propuesta(run.id, "Revisar incidencias").unwrap();
    }

    let (_, i) = get(&f, "/api/inicio").await;
    assert_eq!(i["propuestas"][0]["descripcion"], "Revisar incidencias");
    assert_eq!(i["activas"].as_array().unwrap().len(), 1);
    assert_eq!(i["activas"][0]["id"], ids[1]);
    assert_eq!(i["recientes"].as_array().unwrap().len(), 1);
    assert_eq!(i["recientes"][0]["estado"], "cancelada");
}

#[tokio::test]
async fn repetir_una_tarea_crea_otra_enlazada_a_la_original() {
    let f = montar();
    let (_, t) = post(
        &f,
        "/api/tareas",
        json!({ "titulo": "Informe", "asignado_a": "modelo-local", "solicita": "Resume el día" }),
    )
    .await;
    let id = t["id"].as_i64().unwrap();
    let repetir = format!("/api/tareas/{id}/repetir");

    let (s, _) = post(&f, &repetir, json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "una tarea sin terminar no se repite");
    let (s, _) = post(&f, "/api/tareas/999/repetir", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    post(&f, &format!("/api/tareas/{id}/estado"), json!({ "estado": "cancelada" })).await;
    let (s, nueva) = post(&f, &repetir, json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(nueva["padre"], id);
    assert_eq!(nueva["estado"], "pendiente");
    assert_eq!(nueva["asignado_a"], "modelo-local");
    assert_eq!(nueva["peticion"], "Resume el día");

    let (_, ficha) = get(&f, &format!("/api/tareas/{id}")).await;
    assert_eq!(ficha["hijas"][0]["id"], nueva["id"]);
}

#[tokio::test]
async fn la_ficha_de_una_rutina_enlaza_su_ejecucion_y_repetirla_la_encola() {
    let f = montar();
    let _g = aislar_bus(&f).await;
    // Tarea antigua de una rutina (de antes del bus).
    let (_, t) = post(
        &f,
        "/api/tareas",
        json!({ "titulo": "Ejecutar rutina: revision-diaria", "asignado_a": "orquestador", "solicita": "{\"rutina\":\"revision-diaria\"}" }),
    )
    .await;
    let id = t["id"].as_i64().unwrap();
    {
        let store = f.state.store.lock().await;
        let run = store.start_routine_run("revision-diaria", Some(id)).unwrap();
        store.create_propuesta(run.id, "Hay errores").unwrap();
    }

    let (_, ficha) = get(&f, &format!("/api/tareas/{id}")).await;
    assert_eq!(ficha["ejecucion"]["rutina"], "revision-diaria");
    assert_eq!(ficha["propuestas"][0]["descripcion"], "Hay errores");

    post(&f, &format!("/api/tareas/{id}/estado"), json!({ "estado": "cancelada" })).await;
    let (s, nueva) = post(&f, &format!("/api/tareas/{id}/repetir"), json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(nueva["rutina"], "revision-diaria");
    let (s, _) = post(&f, &format!("/api/tareas/{id}/repetir"), json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT, "ya hay una ejecución en cola");
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
        json!({ "herramienta": "ea_cambiar_estado_tarea", "argumentos": { "id": "uno" } }),
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
