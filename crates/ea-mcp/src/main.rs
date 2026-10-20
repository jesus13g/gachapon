use std::sync::Arc;

use anyhow::Context;
use ea_mcp::EaTools;
use ea_store::Store;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // `red.puerto_mcp` de los ajustes (`EA_MCP_PORT` le gana); el host,
    // siempre 127.0.0.1.
    let efectivos = ea_ajustes::cargar_de(&ea_ajustes::Fuentes::sistema());
    for aviso in &efectivos.avisos {
        tracing::warn!(%aviso, "ajustes");
    }
    let port = efectivos.ajustes.red.puerto_mcp;
    let bind_addr = format!("127.0.0.1:{port}");

    let store = Store::open().context("no se pudo abrir la base de datos")?;
    let store = Arc::new(Mutex::new(store));

    let cancel = tokio_util::sync::CancellationToken::new();
    let service = StreamableHttpService::new(
        move || Ok(EaTools::new(store.clone())),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_cancellation_token(cancel.child_token()),
    );

    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("no se pudo escuchar en {bind_addr}"))?;

    tracing::info!(%bind_addr, "servidor de herramientas MCP arrancado (endpoint /mcp)");

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            cancel.cancel();
        })
        .await?;

    Ok(())
}
