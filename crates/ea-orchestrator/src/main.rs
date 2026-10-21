use std::time::Duration;

use ea_bus::{Bus, ConfigOrquestador};
use ea_orchestrator::config;
use ea_orchestrator::ejecutor::{ejecutar_uno, fabricar};
use ea_store::{EventOutcome, NewEvent, Store};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Actor que este proceso escribe en el diario.
const ACTOR: &str = "orquestador";

/// Bus con historial en SQLite (si la base no abre, sigue solo con ficheros).
fn abrir_bus() -> anyhow::Result<Bus> {
    let bus = Bus::abrir()?;
    Ok(match Store::open() {
        Ok(s) => bus.con_store(s),
        Err(e) => {
            tracing::warn!(error = %e, "bus sin historial SQLite");
            bus
        }
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Avisos de los ajustes (valores inválidos, ficheros rotos), una vez.
    let efectivos = ea_ajustes::cargar_de(&ea_ajustes::Fuentes::sistema());
    for aviso in &efectivos.avisos {
        tracing::warn!(%aviso, "ajustes");
    }
    let config: config::Config = efectivos.ajustes.orquestador.into();

    let intervalo_heartbeat = config.intervalo_heartbeat_s;
    tokio::spawn(async move {
        loop {
            match Store::open() {
                Ok(store) => {
                    if let Err(e) = store.append_event(NewEvent {
                        actor: ACTOR.to_string(),
                        action: "heartbeat".to_string(),
                        trabajo: None,
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

    let bus = abrir_bus()?;
    match bus.recuperar() {
        Ok(v) if !v.is_empty() => tracing::warn!(trabajos = ?v, "trabajos recuperados al arrancar"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "no se pudo recuperar el bus"),
    }
    tracing::info!("ea-orchestrator arrancado");

    let sondeo = Duration::from_secs(config.intervalo_sondeo_s);
    loop {
        // La configuración y el agente del rol ejecutor se rehacen en cada
        // ciclo (un cambio de `roles.ejecutor` vale para el siguiente
        // trabajo); el agente tarda minutos, así que va en un hilo aparte.
        let b = abrir_bus()?;
        let resultado = tokio::task::spawn_blocking(move || {
            let cfg = ConfigOrquestador::cargar();
            let agente = fabricar(&cfg.ejecutor);
            ejecutar_uno(&b, &cfg, agente.as_ref())
        })
        .await;
        match resultado {
            Ok(Some(id)) => {
                tracing::info!(%id, "trabajo procesado");
                continue; // puede haber más en la cola
            }
            Ok(None) => {}
            Err(e) => tracing::error!(error = %e, "el ejecutor falló"),
        }
        tokio::time::sleep(sondeo).await;
    }
}
