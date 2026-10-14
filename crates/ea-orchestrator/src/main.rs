use std::time::Duration;

use ea_orchestrator::config;
use ea_store::{EventOutcome, NewEvent, Store};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Actor que este proceso escribe en el diario.
const ACTOR: &str = "orquestador";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::load();

    let intervalo_heartbeat = config.intervalo_heartbeat_s;
    tokio::spawn(async move {
        loop {
            match Store::open() {
                Ok(store) => {
                    if let Err(e) = store.append_event(NewEvent {
                        actor: ACTOR.to_string(),
                        action: "heartbeat".to_string(),
                        task_id: None,
                        outcome: Some(EventOutcome::Ok),
                        duration_ms: None,
                        detail: None,
                    }) {
                        tracing::warn!(error = %e, "no se pudo registrar el heartbeat");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "no se pudo abrir la base de datos para el heartbeat"),
            }
            tokio::time::sleep(Duration::from_secs(intervalo_heartbeat)).await;
        }
    });

    tracing::info!("ea-orchestrator arrancado (sin ejecutor: en reconstrucción)");

    loop {
        // FASE 3: aquí se enchufará el ejecutor del bus. Cada ciclo leerá
        // las tareas pendientes del bus, las pasará a Qwen Code headless
        // (`qwen -p`) y devolverá el resultado para su revisión.
        tokio::time::sleep(Duration::from_secs(config.intervalo_sondeo_s)).await;
    }
}
