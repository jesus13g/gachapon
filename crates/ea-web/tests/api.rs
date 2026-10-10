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

const RUTINA_EJEMPLO: &str =
    include_str!("../../ea-orchestrator/rutinas-ejemplo/revision-diaria.toml");

struct Fixture {
    state: Arc<AppState>,
    app: Router,
    _dir: TempDir,
}

fn montar() -> Fixture {
    let dir = tempfile::tempdir().expect("directorio temporal");
    let rutinas_dir = dir.path().join("rutinas");
    std::fs::create_dir_all(&rutinas_dir).unwrap();
    std::fs::write(rutinas_dir.join("revision-diaria.toml"), RUTINA_EJEMPLO).unwrap();

    let store = Store::open_at(&dir.path().join("test.db")).expect("abrir store");
    let config = WebConfig {
        puerto: 0,
        voz_puerto: 1, // nadie escucha ahí
        umbral_heartbeat_s: 60,
        tema_path: dir.path().join("no-existe.toml"),
        rutinas_dir,
        boveda: ea_boveda::Boveda::en(dir.path().join("boveda")),
        systemctl: false,
    };
    let state = Arc::new(AppState::new(store, config));
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
        for accion in ["heartbeat", "heartbeat", "voz_escucha_iniciada"] {
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
    assert_eq!(sin[0]["accion"], "voz_escucha_iniciada");

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
async fn rutinas_listar_ver_y_encolar() {
    let f = montar();

    let (_, lista) = get(&f, "/api/rutinas").await;
    assert_eq!(lista[0]["slug"], "revision-diaria");
    assert!(lista[0]["descripcion"].is_string());
    assert!(lista[0]["ultima"].is_null());

    let (s, r) = get(&f, "/api/rutinas/revision-diaria").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["pasos"][0]["id"], r["inicio"]);
    assert_eq!(r["pasos"].as_array().unwrap().len(), 4);

    let (s, t) = post(&f, "/api/rutinas/revision-diaria/encolar", json!({})).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_eq!(t["asignado_a"], "orquestador");
    let peticion: Value = serde_json::from_str(t["peticion"].as_str().unwrap()).unwrap();
    assert_eq!(peticion, json!({ "rutina": "revision-diaria" }));

    let (s, _) = get(&f, "/api/rutinas/no-existe").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = get(&f, "/api/rutinas/a.b").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(&f, "/api/rutinas/a.b/encolar", json!({})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn servicios_y_voz_degradan_con_limpieza() {
    let f = montar();

    let (_, e) = get(&f, "/api/estado").await;
    assert_eq!(e["servicios"].as_array().unwrap().len(), 4);
    assert_eq!(e["servicios"][0]["activo"], "desconocido");
    assert_eq!(e["orquestador"]["vivo"], false);

    let (s, _) = post(&f, "/api/servicios/sshd/reiniciar", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post(&f, "/api/servicios/ea-mcp/reiniciar", json!({})).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);

    let (s, _) = post(&f, "/api/voz/hablar", json!({})).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
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
