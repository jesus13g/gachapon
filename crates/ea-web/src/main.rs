use std::sync::Arc;

use anyhow::Context;
use ea_store::Store;
use ea_web::{clave, AppState, WebConfig};
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
    let estado = AppState::new(store, config).context("no se pudo abrir la conexión de acciones")?;
    let estado = Arc::new(estado);
    let router = ea_web::http::router(estado.clone());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("no se pudo escuchar en {bind_addr}"))?;

    // La clave se escribe después de escuchar: si ya corría otro ea-web, el
    // bind falla antes de pisarle el fichero con una clave que él no conoce.
    // Sin directorio de ejecución el panel queda en solo lectura.
    let fichero_clave = match ea_ajustes::dir_ejecucion(&ea_ajustes::Entorno::Proceso) {
        Some(dir) => match clave::guardar(&dir, &estado.clave) {
            Ok(f) => Some(f),
            Err(e) => {
                tracing::error!(error = %e, "no se pudo guardar la clave de sesión: el panel queda en solo lectura");
                None
            }
        },
        None => {
            tracing::error!("sin directorio de ejecución no hay dónde dejar la clave de sesión: el panel queda en solo lectura");
            None
        }
    };

    tracing::info!(%bind_addr, clave = ?fichero_clave, "ea-web arrancado");

    axum::serve(listener, router)
        .with_graceful_shutdown(ea_plataforma::parada::esperar_parada())
        .await?;

    if let Some(f) = fichero_clave {
        clave::retirar(&f, &estado.clave);
    }
    Ok(())
}
