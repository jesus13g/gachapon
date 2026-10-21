//! La bóveda vista desde el servidor MCP, sobre HTTP de verdad: cada agente
//! escribe solo en su región, ninguno llega a la compartida ni promueve por
//! sí mismo, y lo que se le devuelve al modelo local está acotado. La base de
//! datos y la bóveda son temporales: nunca la bóveda real.

use std::sync::Arc;

use ea_boveda::Boveda;
use ea_mcp::EaTools;
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
    boveda: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn entorno() -> Entorno {
    // Estado del bus en un directorio temporal, nunca el real.
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let estado = ESTADO.get_or_init(|| tempfile::tempdir().unwrap());
    unsafe { std::env::set_var("EA_ESTADO_DIR", estado.path()) };
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let rutinas = dir.path().join("rutinas");
    let boveda = dir.path().join("boveda");
    std::fs::create_dir_all(&rutinas).unwrap();
    let store = Arc::new(Mutex::new(Store::open_at(&db).unwrap()));
    let (r, b) = (rutinas.clone(), boveda.clone());
    let ajustes = ea_ajustes::Fuentes::aislada(&dir.path().join("ajustes"));
    let service = StreamableHttpService::new(
        move || Ok(EaTools::con_entorno(store.clone(), r.clone(), Boveda::en(b.clone())).con_ajustes(ajustes.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Entorno { url: format!("http://{addr}/mcp"), db, boveda, _dir: dir }
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

fn acciones(e: &Entorno) -> Vec<String> {
    Store::open_at(&e.db)
        .unwrap()
        .list_events(&EventFilter { limit: Some(500), ..Default::default() })
        .unwrap()
        .into_iter()
        .map(|x| x.action)
        .collect()
}

#[tokio::test]
async fn cada_agente_escribe_en_su_region_y_el_otro_la_puede_leer() {
    let e = entorno().await;
    let local = conectar(&e.url, "qwen-code-mcp-client").await;
    let claude = conectar(&e.url, "claude-code").await;

    let r = llamar(&local, "ea_escribir_nota", json!({ "titulo": "Puerto del MCP", "contenido": "ea-mcp escucha en el 8765." }))
        .await
        .unwrap();
    assert_eq!(r["ruta"], "modelo-local/puerto-del-mcp.md");
    let r = llamar(&claude, "ea_escribir_nota", json!({ "titulo": "Puerto de la web", "contenido": "ea-web escucha en el 8092.", "etiquetas": ["puertos"], "trabajo": "m-1" }))
        .await
        .unwrap();
    assert_eq!(r["ruta"], "claude-code/puerto-de-la-web.md");

    // Cada uno busca y lee lo del otro.
    let b = llamar(&local, "ea_buscar_notas", json!({ "consulta": "puerto" })).await.unwrap();
    assert_eq!(b["total"], 2);
    let n = llamar(&local, "ea_leer_nota", json!({ "ruta": "claude-code/puerto-de-la-web.md" })).await.unwrap();
    assert_eq!(n["autor"], "claude-code");
    assert_eq!(n["origen"], "trabajo:m-1");
    assert!(n["contenido"].as_str().unwrap().contains("8092"));

    // Y todo queda en el diario atribuido a quien lo hizo.
    let s = Store::open_at(&e.db).unwrap();
    let ev = s.list_events(&EventFilter { limit: Some(100), ..Default::default() }).unwrap();
    let esc: Vec<_> = ev.iter().filter(|x| x.action == "boveda_escritura").map(|x| x.actor.as_str()).collect();
    assert_eq!(esc.len(), 2);
    assert!(esc.contains(&"modelo-local") && esc.contains(&"claude-code"));
}

#[tokio::test]
async fn un_agente_no_puede_escribir_en_la_region_compartida_ni_fuera_de_la_boveda() {
    let e = entorno().await;
    for cliente in ["qwen-code-mcp-client", "claude-code"] {
        let c = conectar(&e.url, cliente).await;
        for ruta in ["../compartida/x.md", "/tmp/ea-fuera.md", "a/../../compartida/x.md"] {
            let r = llamar(&c, "ea_escribir_nota", json!({ "titulo": "T", "contenido": "c", "ruta": ruta })).await;
            assert!(r.is_err(), "{cliente} {ruta}: {r:?}");
        }
        // La herramienta no tiene argumento de región, y uno inventado no cuenta.
        let r = llamar(&c, "ea_escribir_nota", json!({ "titulo": "T", "contenido": "c", "region": "compartida" }))
            .await
            .unwrap();
        assert!(r["ruta"].as_str().unwrap().starts_with(cliente_region(cliente)), "{r}");
    }
    assert!(!e.boveda.join("compartida").exists());
    assert!(!std::path::Path::new("/tmp/ea-fuera.md").exists());
}

fn cliente_region(cliente: &str) -> &'static str {
    if cliente == "qwen-code-mcp-client" { "modelo-local/" } else { "claude-code/" }
}

#[tokio::test]
async fn el_orquestador_desatendido_solo_lee() {
    let e = entorno().await;
    let o = conectar(&e.url, "ea-orchestrator").await;
    let r = llamar(&o, "ea_escribir_nota", json!({ "titulo": "T", "contenido": "c" })).await;
    assert!(r.unwrap_err().contains("no está disponible"));
    let r = llamar(&o, "ea_promover_nota", json!({ "ruta": "modelo-local/x.md" })).await;
    assert!(r.is_err());
    // Leer sí puede.
    let r = llamar(&o, "ea_buscar_notas", json!({ "consulta": "nada" })).await.unwrap();
    assert_eq!(r["total"], 0);
    assert!(!e.boveda.join("modelo-local").exists());
    assert!(acciones(&e).contains(&"mcp_escribir_nota_denegada".to_string()));
}

#[tokio::test]
async fn un_agente_solo_propone_la_promocion_y_no_llega_nada_a_la_compartida() {
    let e = entorno().await;
    let c = conectar(&e.url, "qwen-code-mcp-client").await;
    let n = llamar(&c, "ea_escribir_nota", json!({ "titulo": "Hallazgo", "contenido": "Algo útil." })).await.unwrap();
    let ruta = n["ruta"].as_str().unwrap().to_string();

    let sim = llamar(&c, "ea_promover_nota", json!({ "ruta": ruta, "simular": true })).await.unwrap();
    assert_eq!(sim["simulado"], true);
    assert!(sim["simulacion"].as_str().unwrap().contains("compartida/hallazgo.md"));
    assert!(!e.boveda.join("compartida").exists());

    let r = llamar(&c, "ea_promover_nota", json!({ "ruta": ruta, "confirmar": true })).await.unwrap();
    assert_eq!(r["ejecutado"], false);
    assert_eq!(r["propuesta"]["estado"], "pendiente");
    assert!(!e.boveda.join("compartida").exists(), "promover no copia nada");

    let ps = Store::open_at(&e.db).unwrap().list_propuestas(&PropuestaFilter::default()).unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].estado, EstadoPropuesta::Pendiente);
    assert_eq!(ps[0].proponente.as_deref(), Some("modelo-local"));
    assert_eq!(ps[0].accion.as_ref().unwrap().herramienta, "ea_promover_nota");

    // Rutas hostiles no llegan ni a propuesta.
    for mala in ["../x.md", "compartida/x.md", "/etc/passwd"] {
        assert!(llamar(&c, "ea_promover_nota", json!({ "ruta": mala })).await.is_err(), "{mala}");
    }
    assert_eq!(Store::open_at(&e.db).unwrap().list_propuestas(&PropuestaFilter::default()).unwrap().len(), 1);
}

#[tokio::test]
async fn lo_que_recibe_el_modelo_local_esta_acotado() {
    let e = entorno().await;
    let claude = conectar(&e.url, "claude-code").await;
    let local = conectar(&e.url, "qwen-code-mcp-client").await;
    let largo = "relleno de la nota ".repeat(1500); // ~28 KB
    for i in 0..12 {
        llamar(&claude, "ea_escribir_nota", json!({ "titulo": format!("Larga {i}"), "contenido": largo }))
            .await
            .unwrap();
    }
    // El modelo local no puede escribir más de su cota.
    let r = llamar(&local, "ea_escribir_nota", json!({ "titulo": "Enorme", "contenido": largo })).await;
    assert!(r.is_err());

    let b = llamar(&local, "ea_buscar_notas", json!({ "consulta": "relleno", "limite": 50 })).await.unwrap();
    assert_eq!(b["resultados"].as_array().unwrap().len(), 5);
    let bc = llamar(&claude, "ea_buscar_notas", json!({ "consulta": "relleno", "limite": 50 })).await.unwrap();
    assert_eq!(bc["resultados"].as_array().unwrap().len(), 12);

    let n = llamar(&local, "ea_leer_nota", json!({ "ruta": "claude-code/larga-0.md" })).await.unwrap();
    assert_eq!(n["contenido"].as_str().unwrap().chars().count(), 3000);
    assert_eq!(n["truncado"], true);
    let n2 = llamar(&local, "ea_leer_nota", json!({ "ruta": "claude-code/larga-0.md", "desde": n["siguiente_desde"].clone() }))
        .await
        .unwrap();
    assert_eq!(n2["desde"], 3000);
    // Y el tamaño total de lo que se serializa al modelo cabe de sobra en su contexto.
    assert!(serde_json::to_string(&b).unwrap().len() < 3000);
    assert!(serde_json::to_string(&n).unwrap().len() < 4500);
}

#[tokio::test]
async fn escribir_admite_simulacion() {
    let e = entorno().await;
    let c = conectar(&e.url, "claude-code").await;
    let r = llamar(&c, "ea_escribir_nota", json!({ "titulo": "Prueba", "contenido": "x", "simular": true })).await.unwrap();
    assert_eq!(r["simulado"], true);
    assert_eq!(r["ruta"], "claude-code/prueba.md");
    assert!(!e.boveda.join("claude-code").exists());
    assert!(acciones(&e).contains(&"mcp_escribir_nota_simulada".to_string()));
}
