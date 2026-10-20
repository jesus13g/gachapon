//! Mutaciones supervisadas vistas desde el servidor MCP, sobre HTTP de
//! verdad: un agente solo puede simular o encolar una propuesta; nunca
//! ejecutar. La base de datos y el directorio de rutinas son temporales.

use std::sync::Arc;

use ea_acciones::{Riesgo, CATALOGO};
use ea_mcp::EaTools;
use ea_boveda::Boveda;
use ea_store::{EstadoPropuesta, EventFilter, PropuestaFilter, Store};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
    ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use serde_json::{json, Value};
use tokio::sync::Mutex;

type Client = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

struct Entorno {
    url: String,
    db: std::path::PathBuf,
    rutinas: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn entorno() -> Entorno {
    // Estado del bus y bóveda en directorios temporales, nunca los reales.
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let estado = ESTADO.get_or_init(|| tempfile::tempdir().unwrap());
    unsafe { std::env::set_var("EA_ESTADO_DIR", estado.path()) };
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let rutinas = dir.path().join("rutinas");
    std::fs::create_dir_all(&rutinas).unwrap();
    std::fs::write(rutinas.join("vieja.toml"), "[rutina]\n").unwrap();
    let store = Arc::new(Mutex::new(Store::open_at(&db).unwrap()));
    let r = rutinas.clone();
    let boveda = dir.path().join("boveda");
    let service = StreamableHttpService::new(
        move || Ok(EaTools::con_entorno(store.clone(), r.clone(), Boveda::en(boveda.clone()))),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Entorno { url: format!("http://{addr}/mcp"), db, rutinas, _dir: dir }
}

async fn conectar(url: &str, nombre: &str) -> Client {
    InitializeRequestParams::new(ClientCapabilities::default(), Implementation::new(nombre, "0.0.1"))
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_uri(url.to_string()),
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .unwrap()
}

/// Devuelve `Ok(json)` o `Err(mensaje)` si la herramienta se rechazó.
async fn llamar(c: &Client, nombre: &str, args: Value) -> Result<Value, String> {
    let r = c
        .call_tool(
            CallToolRequestParams::new(nombre.to_string())
                .with_arguments(args.as_object().cloned().unwrap_or_default()),
        )
        .await
        .map_err(|e| e.to_string())?;
    let t = &r.content[0].as_text().unwrap().text;
    Ok(serde_json::from_str(t).unwrap_or(Value::String(t.clone())))
}

fn propuestas(e: &Entorno) -> Vec<ea_store::Propuesta> {
    Store::open_at(&e.db).unwrap().list_propuestas(&PropuestaFilter::default()).unwrap()
}

#[tokio::test]
async fn el_catalogo_mcp_coincide_con_el_catalogo_y_no_hay_forma_de_confirmar() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let tools = c.list_tools(Default::default()).await.unwrap().tools;
    let mut expuestas: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    let mut esperadas: Vec<String> = CATALOGO.iter().map(|h| h.nombre.to_string()).collect();
    expuestas.sort();
    esperadas.sort();
    assert_eq!(expuestas, esperadas, "una sola fuente de verdad: el catálogo");
    // Ninguna herramienta decide propuestas ni confirma nada.
    for n in &expuestas {
        for prohibido in ["decidir", "aceptar", "confirmar", "propuesta", "ejecutar"] {
            assert!(!n.contains(prohibido), "{n}");
        }
    }
    // Las anotaciones MCP reflejan el nivel de riesgo declarado.
    for t in &tools {
        let h = ea_acciones::buscar(&t.name).unwrap();
        let a = t.annotations.as_ref().expect("anotaciones");
        assert_eq!(a.read_only_hint, Some(h.riesgo == Riesgo::Lectura), "{}", t.name);
        if h.riesgo == Riesgo::Irreversible {
            assert_eq!(a.destructive_hint, Some(true), "{}", t.name);
        }
    }
}

#[tokio::test]
async fn simular_no_escribe_nada_pero_queda_en_el_diario() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, "ea_promover_nota", json!({ "ruta": "claude-code/inexistente.md", "simular": true })).await;
    assert!(r.is_err(), "una nota inexistente no se simula");
    let r = llamar(&c, "ea_eliminar_rutina", json!({ "slug": "vieja", "simular": true })).await.unwrap();
    assert_eq!(r["riesgo"], "irreversible");

    assert!(propuestas(&e).is_empty());
    assert!(e.rutinas.join("vieja.toml").exists());
    let s = Store::open_at(&e.db).unwrap();
    let ev = s.list_events(&EventFilter { limit: Some(50), ..Default::default() }).unwrap();
    let sim = ev.iter().find(|x| x.action == "mcp_eliminar_rutina_simulada").expect("diario");
    assert_eq!(sim.actor, "claude-code");
    assert!(sim.detail.as_ref().unwrap().contains("vieja"));
    assert!(ev.iter().any(|x| x.action == "mcp_eliminar_rutina_simulada"));
}

#[tokio::test]
async fn un_agente_que_pide_ejecutar_solo_consigue_una_propuesta_pendiente() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    // Aunque el agente intente "confirmarse" con argumentos de su cosecha.
    let r = llamar(
        &c,
        "ea_eliminar_rutina",
        json!({ "slug": "vieja", "confirmar": true, "confirmacion": "si", "token": "x" }),
    )
    .await
    .unwrap();
    assert_eq!(r["ejecutado"], false);
    assert_eq!(r["propuesta"]["estado"], "pendiente");
    assert_eq!(r["riesgo"], "irreversible");
    assert!(r.get("token").is_none() && r.get("confirmacion").is_none());
    assert!(r["aviso"].as_str().unwrap().contains("NO se ha ejecutado"));
    assert!(r["aviso"].as_str().unwrap().contains("terminal"));

    let ps = propuestas(&e);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].estado, EstadoPropuesta::Pendiente);
    assert_eq!(ps[0].proponente.as_deref(), Some("claude-code"));
    let a = ps[0].accion.as_ref().unwrap();
    assert_eq!(a.herramienta, "ea_eliminar_rutina");
    assert!(a.argumentos.get("confirmar").is_none());
    assert!(ps[0].ejecucion.is_none());
    assert!(e.rutinas.join("vieja.toml").exists(), "el fichero sigue ahí");

    // Repetir no duplica.
    let r2 = llamar(&c, "ea_eliminar_rutina", json!({ "slug": "vieja" })).await.unwrap();
    assert_eq!(r2["propuesta"]["id"], r["propuesta"]["id"]);
    assert_eq!(r2["propuesta"]["nueva"], false);
    assert_eq!(propuestas(&e).len(), 1);
}

#[tokio::test]
async fn el_modelo_local_como_mucho_propone_reversibles_de_su_proyeccion() {
    let e = entorno().await;
    let c = conectar(&e.url, "qwen-code-mcp-client").await;
    // Una nota suya (en una bóveda temporal) que propone promover.
    let r = llamar(&c, "ea_escribir_nota", json!({ "titulo": "Hallazgo", "contenido": "algo" })).await.unwrap();
    let ruta = r["ruta"].as_str().unwrap().to_string();
    let r = llamar(&c, "ea_promover_nota", json!({ "ruta": ruta })).await.unwrap();
    assert_eq!(r["ejecutado"], false);
    assert_eq!(propuestas(&e)[0].proponente.as_deref(), Some("modelo-local"));

    for (n, a) in [
        ("ea_eliminar_rutina", json!({ "slug": "vieja", "simular": true })),
        ("ea_reiniciar_servicio", json!({ "servicio": "ea-mcp", "simular": true })),
        ("ea_encolar_rutina", json!({ "slug": "vieja", "simular": true })),
        ("ea_registrar_rutina", json!({ "toml": "[rutina]", "simular": true })),
    ] {
        let err = llamar(&c, n, a).await.unwrap_err();
        assert!(err.contains("no está disponible"), "{n}: {err}");
    }
    assert_eq!(propuestas(&e).len(), 1);
    assert!(e.rutinas.join("vieja.toml").exists());
}

#[tokio::test]
async fn lo_desatendido_no_puede_ni_simular_ni_proponer() {
    let e = entorno().await;
    let c = conectar(&e.url, "ea-orchestrator").await;
    // Las lecturas siguen funcionando.
    llamar(&c, "ea_status", json!({})).await.unwrap();
    for h in CATALOGO.iter().filter(|h| h.riesgo != Riesgo::Lectura) {
        let args = match h.nombre {
            "ea_reiniciar_servicio" => json!({ "servicio": "ea-mcp" }),
            // Denegadas antes de tocar nada, así que no llegan a la bóveda configurada.
            "ea_escribir_nota" => json!({ "titulo": "a", "contenido": "b" }),
            "ea_promover_nota" => json!({ "ruta": "modelo-local/a.md" }),
            "ea_registrar_rutina" => json!({ "toml": "[rutina]" }),
            _ => json!({ "slug": "vieja" }),
        };
        let err = llamar(&c, h.nombre, args).await.unwrap_err();
        assert!(err.contains("no está disponible"), "{}: {err}", h.nombre);
    }
    assert!(propuestas(&e).is_empty());
    assert!(e.rutinas.join("vieja.toml").exists());
}

#[tokio::test]
async fn argumentos_fuera_de_la_lista_cerrada_se_rechazan() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    for (n, a) in [
        ("ea_reiniciar_servicio", json!({ "servicio": "sshd" })),
        ("ea_eliminar_rutina", json!({ "slug": "../vieja" })),
        ("ea_eliminar_rutina", json!({ "slug": "no-existe" })),
    ] {
        assert!(llamar(&c, n, a).await.is_err(), "{n}");
    }
    assert!(propuestas(&e).is_empty());
}
