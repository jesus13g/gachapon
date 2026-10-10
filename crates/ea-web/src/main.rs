use std::sync::Arc;

use anyhow::Context;
use ea_store::Store;
use ea_web::{api, AppState, WebConfig};
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

    let config = WebConfig::load();
    let store = Store::open().context("no se pudo abrir la base de datos")?;

    let bind_addr = format!("127.0.0.1:{}", config.puerto);
    let router = api::router(Arc::new(AppState::new(store, config)));
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("no se pudo escuchar en {bind_addr}"))?;

    tracing::info!(%bind_addr, "ea-web arrancado");

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;

    Ok(())
}
