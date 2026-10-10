use std::sync::Arc;

use anyhow::Context;
use ea_proxy::{config, http, llama_client, mcp_client};
use ea_store::Store;
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const ENV_PORT: &str = "EA_PROXY_PORT";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut config = config::load();
    if let Some(port) = std::env::var(ENV_PORT).ok().and_then(|s| s.parse().ok()) {
        config.puerto = port;
    }

    let store = Store::open().context("no se pudo abrir la base de datos")?;
    let llama = llama_client::LlamaClient::new(config.llama_endpoint.clone());
    let mcp = mcp_client::McpClient::connect(&config.mcp_endpoint)
        .await
        .context("no se pudo conectar con ea-mcp")?;

    let bind_addr = format!("127.0.0.1:{}", config.puerto);
    let state = Arc::new(http::AppState {
        llama,
        mcp,
        store: Mutex::new(store),
        config,
    });

    let router = http::router(state);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("no se pudo escuchar en {bind_addr}"))?;

    tracing::info!(%bind_addr, "ea-proxy arrancado");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
