use std::sync::Arc;

use anyhow::Context;
use ea_mcp::EaTools;
use ea_store::Store;
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

    let store = Store::open_at(&efectivos.ajustes.rutas.db).context("no se pudo abrir la base de datos")?;
    let store = Arc::new(Mutex::new(store));

    let cancel = tokio_util::sync::CancellationToken::new();
    let router = ea_mcp::router(move || EaTools::new(store.clone()), cancel.child_token());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("no se pudo escuchar en {bind_addr}"))?;

    tracing::info!(%bind_addr, "servidor de herramientas MCP arrancado (endpoint /mcp)");

    // SIGTERM (systemd, launchd) o Ctrl-C: deja de aceptar, cierra las
    // sesiones abiertas (sus flujos SSE no acabarían solos) y sale con 0.
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            ea_plataforma::parada::esperar_parada().await;
            cancel.cancel();
        })
        .await?;

    Ok(())
}
