use std::time::Duration;

use ea_bus::Bus;
use ea_orchestrator::config;
use ea_orchestrator::trabajadores::{Contexto, Trabajadores};
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

    // Un trabajador por ejecutor despierto (spec 019): los miembros ejecutor de
    // los equipos despiertos o, sin equipos, solo `roles.ejecutor`. Cada
    // trabajador rehace su configuración y su adaptador en cada ciclo y el
    // agente tarda minutos, así que van en hilos aparte; aquí solo se
    // reconcilia quién debe tener trabajador.
    let sondeo = Duration::from_secs(config.intervalo_sondeo_s);
    let mut trabajadores = Trabajadores::new(Contexto::sistema(|| abrir_bus().ok(), sondeo));
    loop {
        trabajadores.reconciliar();
        tokio::time::sleep(sondeo).await;
    }
}
