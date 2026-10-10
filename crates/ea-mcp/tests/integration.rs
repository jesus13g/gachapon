//! Cableado MCP de extremo a extremo, sobre HTTP de verdad. La lógica de
//! negocio (repositorios de tareas/eventos) ya está cubierta por los tests
//! de `ea-store`; esto solo comprueba que el servidor expone el catálogo
//! esperado y que una llamada real de ida y vuelta funciona.

use std::sync::Arc;

use ea_mcp::EaTools;
use ea_store::Store;
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
    ProtocolVersion,
};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ClientLifecycleMode, ClientServiceExt};
use tokio::sync::Mutex;

type Client = rmcp::service::RunningService<rmcp::RoleClient, InitializeRequestParams>;

async fn connect(url: String) -> Client {
    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("ea-mcp-test-client", "0.0.1"),
    );
    client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .expect("conectar al servidor MCP de prueba")
}

fn text_of(result: &rmcp::model::CallToolResult) -> serde_json::Value {
    let text = &result.content[0].as_text().expect("contenido de texto").text;
    serde_json::from_str(text).expect("contenido JSON válido")
}

async fn start_server(db_path: std::path::PathBuf) -> String {
    let store = Store::open_at(&db_path).expect("abrir store de prueba");
    let store = Arc::new(Mutex::new(store));

    let service = StreamableHttpService::new(
        move || Ok(EaTools::new(store.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind puerto efímero");
    let addr = listener.local_addr().expect("local_addr");

    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    format!("http://{addr}/mcp")
}

#[tokio::test]
async fn expone_el_catalogo_y_responde_a_una_llamada() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;
    let client = connect(url).await;

    let tools = client
        .list_tools(Default::default())
        .await
        .expect("tools/list");
    let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
    for expected in [
        "ea_status",
        "ea_listar_tareas",
        "ea_ver_tarea",
        "ea_listar_diario",
        "ea_crear_tarea",
    ] {
        assert!(
            names.contains(&expected),
            "falta la herramienta {expected} en el catálogo: {names:?}"
        );
    }

    let status = client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status");
    assert_ne!(status.content.len(), 0, "ea_status no devolvió contenido");

    client.cancel().await.ok();
}

#[tokio::test]
async fn atribuye_al_modelo_local_cuando_el_cliente_es_ea_proxy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;

    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("ea-proxy", "0.0.1"),
    );
    let client = client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .expect("conectar como ea-proxy");

    client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status como ea-proxy");

    let diario = client
        .call_tool(CallToolRequestParams::new("ea_listar_diario"))
        .await
        .expect("listar diario");
    let diario = text_of(&diario);
    let actores: Vec<&str> = diario["eventos"]
        .as_array()
        .expect("eventos")
        .iter()
        .map(|e| e["actor"].as_str().unwrap_or_default())
        .collect();
    assert!(
        actores.contains(&"modelo-local"),
        "las invocaciones de ea-proxy deben quedar atribuidas a modelo-local: {actores:?}"
    );
    assert!(
        !actores.contains(&"claude-code"),
        "no debe atribuirse a claude-code lo que hizo ea-proxy: {actores:?}"
    );

    client.cancel().await.ok();
}

#[tokio::test]
async fn atribuye_al_orquestador_cuando_el_cliente_es_ea_orchestrator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;

    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("ea-orchestrator", "0.0.1"),
    );
    let client = client_info
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::V_2025_11_25],
                legacy_version: None,
            },
        )
        .await
        .expect("conectar como ea-orchestrator");

    client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status como ea-orchestrator");

    let diario = client
        .call_tool(CallToolRequestParams::new("ea_listar_diario"))
        .await
        .expect("listar diario");
    let diario = text_of(&diario);
    let actores: Vec<&str> = diario["eventos"]
        .as_array()
        .expect("eventos")
        .iter()
        .map(|e| e["actor"].as_str().unwrap_or_default())
        .collect();
    assert!(
        actores.contains(&"orquestador"),
        "las invocaciones de ea-orchestrator deben quedar atribuidas a orquestador: {actores:?}"
    );
    assert!(
        !actores.contains(&"claude-code"),
        "no debe atribuirse a claude-code lo que hizo ea-orchestrator: {actores:?}"
    );

    client.cancel().await.ok();
}

#[tokio::test]
async fn crear_tarea_simula_sin_escribir_y_luego_crea_de_verdad() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;
    let client = connect(url).await;

    let mut args = serde_json::Map::new();
    args.insert("titulo".into(), serde_json::json!("Probar MCP"));
    args.insert("solicita".into(), serde_json::json!("Verificar simular"));
    args.insert("asignado_a".into(), serde_json::json!("modelo-local"));
    args.insert("simular".into(), serde_json::json!(true));

    let simulated = client
        .call_tool(CallToolRequestParams::new("ea_crear_tarea").with_arguments(args.clone()))
        .await
        .expect("llamar a ea_crear_tarea (simular)");
    let simulated = text_of(&simulated);
    assert_eq!(simulated["simulado"], serde_json::json!(true));

    let listado = client
        .call_tool(CallToolRequestParams::new("ea_listar_tareas"))
        .await
        .expect("listar tareas tras simular");
    let listado = text_of(&listado);
    assert_eq!(
        listado["tareas"].as_array().map(Vec::len),
        Some(0),
        "simular=true no debe escribir ninguna tarea"
    );

    let mut args_reales = args;
    args_reales.insert("simular".into(), serde_json::json!(false));
    let creada = client
        .call_tool(CallToolRequestParams::new("ea_crear_tarea").with_arguments(args_reales))
        .await
        .expect("llamar a ea_crear_tarea (real)");
    let creada = text_of(&creada);
    assert_eq!(creada["tarea"]["origen"], serde_json::json!("agente"));
    assert_eq!(creada["tarea"]["creada_por"], serde_json::json!("claude-code"));

    let listado = client
        .call_tool(CallToolRequestParams::new("ea_listar_tareas"))
        .await
        .expect("listar tareas tras crear de verdad");
    let listado = text_of(&listado);
    assert_eq!(listado["tareas"].as_array().map(Vec::len), Some(1));

    let diario = client
        .call_tool(CallToolRequestParams::new("ea_listar_diario"))
        .await
        .expect("listar diario");
    let diario = text_of(&diario);
    let acciones: Vec<&str> = diario["eventos"]
        .as_array()
        .expect("eventos")
        .iter()
        .map(|e| e["accion"].as_str().unwrap_or_default())
        .collect();
    for esperada in [
        "mcp_crear_tarea_simulada",
        "tarea_creada",
        "mcp_listar_tareas",
    ] {
        assert!(
            acciones.contains(&esperada),
            "falta la acción '{esperada}' en el diario: {acciones:?}"
        );
    }

    client.cancel().await.ok();
}

#[tokio::test]
async fn ea_status_informa_del_estado_real_del_orquestador() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path.clone()).await;
    let client = connect(url).await;

    let sin_latido = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_status"))
            .await
            .expect("llamar a ea_status"),
    );
    assert_eq!(sin_latido["orquestador"]["vivo"], false);
    assert!(sin_latido["orquestador"]["ultimo_latido"].is_null());

    // Otra conexión al mismo fichero hace de orquestador emitiendo su latido.
    let otro = Store::open_at(&db_path).expect("segunda conexión");
    otro.append_event(ea_store::NewEvent {
        actor: "orquestador".into(),
        action: "heartbeat".into(),
        task_id: None,
        outcome: None,
        duration_ms: None,
        detail: None,
    })
    .expect("registrar latido");

    let con_latido = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_status"))
            .await
            .expect("llamar a ea_status"),
    );
    assert_eq!(con_latido["orquestador"]["vivo"], true);
    assert_eq!(con_latido["orquestador"]["estado"], "vivo");
    assert!(con_latido["orquestador"]["ultimo_latido"].is_string());

    client.cancel().await.ok();
}
