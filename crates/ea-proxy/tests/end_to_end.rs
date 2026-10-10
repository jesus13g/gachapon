//! Prueba de extremo a extremo real: clasifica → elige herramienta →
//! extrae argumentos → ejecuta contra un `ea-mcp` efímero, contra el
//! `llama-server` de verdad en `127.0.0.1:8080`. No se mete en el
//! `cargo test` por defecto (depende de un servicio externo pesado, igual
//! que ningún test de fase 1/2 dependía de él) — se corre a mano con:
//!
//!     cargo test -p ea-proxy -- --ignored
//!
//! con `llama-server` arriba (`systemctl --user status llama-server`).

use std::sync::Arc;

use ea_mcp::EaTools;
use ea_proxy::config::Config;
use ea_proxy::decide;
use ea_proxy::llama_client::{ChatMessage, LlamaClient};
use ea_proxy::mcp_client::McpClient;
use ea_store::Store;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::sync::Mutex;

async fn start_ea_mcp(db_path: std::path::PathBuf) -> String {
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
#[ignore]
async fn flujo_completo_contra_llama_server_real() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Misma base de datos para el ea-mcp efímero y para el store que usa
    // ea-proxy para su propio diario: en producción ambos procesos abren
    // la misma ruta XDG, así que el test debe reflejar eso para que las
    // aserciones sobre el diario vean todos los eventos (los del propio
    // proxy y los que ea-mcp registra al ejecutar cada herramienta).
    let db_path = dir.path().join("ecosistema-test.db");

    let mcp_url = start_ea_mcp(db_path.clone()).await;
    let mcp = McpClient::connect(&mcp_url).await.expect("conectar a ea-mcp efímero");
    let llama = LlamaClient::new("http://127.0.0.1:8080".to_string());
    let store = Mutex::new(Store::open_at(&db_path).expect("abrir store de prueba"));
    let profile = Config::default().perfiles["rapido"].clone();

    // Caso sin herramienta.
    let saludo = [ChatMessage::user("Hola, ¿qué tal estás?")];
    let decision = decide::decidir(&llama, &mcp, &store, &saludo, &profile)
        .await
        .expect("decidir sobre saludo");
    assert!(
        decision.herramienta_usada.is_none(),
        "un saludo no debe disparar ninguna herramienta: {:?}",
        decision.herramienta_usada
    );
    assert!(!decision.content.is_empty());

    // Caso con herramienta sin argumentos.
    let estado = [ChatMessage::user("Dame el estado general del ecosistema")];
    let decision = decide::decidir(&llama, &mcp, &store, &estado, &profile)
        .await
        .expect("decidir sobre estado");
    assert_eq!(decision.herramienta_usada.as_deref(), Some("ea_status"));

    // Caso con herramienta y argumento enum.
    let tareas = [ChatMessage::user(
        "Enséñame las tareas completadas, máximo 3",
    )];
    let decision = decide::decidir(&llama, &mcp, &store, &tareas, &profile)
        .await
        .expect("decidir sobre listar tareas");
    assert_eq!(decision.herramienta_usada.as_deref(), Some("ea_listar_tareas"));
    let args = decision.argumentos_usados.expect("debe haber argumentos");
    assert_eq!(args["estado"], "completada");

    // El diario debe reflejar los tres pasos de decisión, atribuidos al
    // modelo local, más las ejecuciones reales contra ea-mcp (que ahora
    // también se atribuyen a modelo-local vía actor_for).
    let store = store.lock().await;
    let eventos = store
        .list_events(&Default::default())
        .expect("listar diario");
    let acciones: Vec<&str> = eventos.iter().map(|e| e.action.as_str()).collect();
    for esperada in ["proxy_clasificacion", "proxy_decision_herramienta"] {
        assert!(
            acciones.iter().filter(|a| **a == esperada).count() >= 2,
            "esperaba al menos 2 ocurrencias de '{esperada}' (2 de los 3 casos usan \
             herramienta): {acciones:?}"
        );
    }
}

#[tokio::test]
#[ignore]
async fn elegir_rama_contra_llama_server_real() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("ecosistema-test.db");
    let store = Mutex::new(Store::open_at(&db_path).expect("abrir store de prueba"));
    let llama = LlamaClient::new("http://127.0.0.1:8080".to_string());

    let opciones = vec!["hay_errores".to_string(), "sin_errores".to_string()];
    let rama = decide::elegir_rama(
        &llama,
        &store,
        "Eventos del diario: ninguno con outcome=error. ¿Hay algún evento con outcome=error?",
        &opciones,
    )
    .await
    .expect("elegir rama");
    assert!(opciones.contains(&rama), "rama fuera del conjunto cerrado: {rama}");
}
