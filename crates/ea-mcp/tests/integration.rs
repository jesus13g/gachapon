//! Cableado MCP de extremo a extremo, sobre HTTP de verdad. La lógica de
//! negocio (repositorios de eventos) ya está cubierta por los tests
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

/// Estado del bus en un directorio temporal compartido por el proceso de
/// pruebas, nunca el real.
fn estado_temporal() -> &'static std::path::Path {
    static ESTADO: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    let dir = ESTADO.get_or_init(|| tempfile::tempdir().unwrap());
    unsafe { std::env::set_var("EA_ESTADO_DIR", dir.path()) };
    dir.path()
}

async fn start_server(db_path: std::path::PathBuf) -> String {
    estado_temporal();
    let store = Store::open_at(&db_path).expect("abrir store de prueba");
    let store = Arc::new(Mutex::new(store));
    let rutinas = db_path.parent().unwrap().join("rutinas");

    let service = StreamableHttpService::new(
        move || Ok(EaTools::con_rutinas_dir(store.clone(), rutinas.clone())),
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
    for ausente in ["ea_listar_tareas", "ea_ver_tarea", "ea_crear_tarea", "ea_cambiar_estado_tarea"] {
        assert!(!names.contains(&ausente), "{ausente} ya no existe");
    }
    // Los ajustes no existen para los agentes: ni leerlos ni cambiarlos.
    assert!(!names.iter().any(|n| n.contains("ajuste")), "{names:?}");
    for expected in [
        "ea_status",
        "ea_listar_trabajos",
        "ea_ver_trabajo",
        "ea_listar_diario",
        "ea_encolar_rutina",
        "ea_registrar_rutina",
        "ea_reiniciar_servicio",
        "ea_eliminar_rutina",
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
async fn atribuye_al_modelo_local_cuando_el_cliente_es_qwen_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;

    let transport = StreamableHttpClientTransport::from_uri(url);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("qwen-code-mcp-client", "0.0.1"),
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
        .expect("conectar como qwen-code");

    client
        .call_tool(CallToolRequestParams::new("ea_status"))
        .await
        .expect("llamar a ea_status como qwen-code");

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
        "las invocaciones de qwen-code deben quedar atribuidas a modelo-local: {actores:?}"
    );
    assert!(
        !actores.contains(&"claude-code"),
        "no debe atribuirse a claude-code lo que hizo qwen-code: {actores:?}"
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
async fn los_trabajos_del_bus_se_leen_pero_no_se_crean_desde_mcp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let url = start_server(db_path).await;
    let client = connect(url).await;

    // Un trabajo creado por el bus (como haría `ea trabajo crear`).
    let wt = tempfile::tempdir().expect("worktree");
    let bus = ea_bus::Bus::abrir().expect("bus temporal");
    let id = bus
        .crear_trabajo(ea_bus::Orden {
            id: String::new(),
            modo: ea_bus::Modo::Mision,
            titulo: "Probar MCP".into(),
            worktree: wt.path().to_string_lossy().into_owned(),
            objetivo: "Verificar la lectura".into(),
            criterio_exito: Some("true".into()),
            fases: vec![],
            revisar_cada: None,
            topes: ea_bus::Topes::default(),
            rutina: None,
            notas_revision: vec![],
            skills: vec![],
            creada_en: String::new(),
            creada_por: "test".into(),
        })
        .expect("crear trabajo");

    let listado = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_listar_trabajos"))
            .await
            .expect("listar trabajos"),
    );
    let ids: Vec<_> = listado["trabajos"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.contains(&id), "{ids:?}");

    let mut args = serde_json::Map::new();
    args.insert("id".into(), serde_json::json!(id));
    let detalle = text_of(
        &client
            .call_tool(CallToolRequestParams::new("ea_ver_trabajo").with_arguments(args))
            .await
            .expect("ver trabajo"),
    );
    assert_eq!(detalle["orden"]["titulo"], "Probar MCP");
    assert!(detalle["informe"].is_null());

    let mut args = serde_json::Map::new();
    args.insert("id".into(), serde_json::json!("no-existe"));
    assert!(client.call_tool(CallToolRequestParams::new("ea_ver_trabajo").with_arguments(args)).await.is_err());

    let status = text_of(&client.call_tool(CallToolRequestParams::new("ea_status")).await.unwrap());
    assert!(status["trabajos_por_estado"].is_object());
    assert!(status["colas"]["modelo-local"].is_array());

    let diario = text_of(&client.call_tool(CallToolRequestParams::new("ea_listar_diario")).await.unwrap());
    let acciones: Vec<&str> = diario["eventos"].as_array().unwrap().iter().map(|e| e["accion"].as_str().unwrap_or_default()).collect();
    for esperada in ["mcp_listar_trabajos", "mcp_ver_trabajo", "mcp_status"] {
        assert!(acciones.contains(&esperada), "falta '{esperada}': {acciones:?}");
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
        trabajo: None,
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
