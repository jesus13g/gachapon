use std::time::Duration;

use ea_bus::Bus;
use ea_orchestrator::trabajadores::{arrancar, Contexto, Trabajadores};
use ea_store::{EventOutcome, NewEvent, Store};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Actor que este proceso escribe en el diario.
const ACTOR: &str = ea_ajustes::ACTOR_ORQUESTADOR;

/// Bus con historial en SQLite (si la base no abre, sigue solo con ficheros),
/// en las rutas de los ajustes de ahora.
fn abrir_bus() -> anyhow::Result<Bus> {
    let rutas = ea_ajustes::cargar().rutas;
    let bus = Bus::abrir_en(&rutas.estado)?;
    Ok(match Store::open_at(&rutas.db) {
        Ok(s) => bus.con_store(s, ACTOR),
        Err(e) => {
            tracing::warn!(error = %e, "bus sin historial SQLite");
            bus
        }
    })
}

/// Un evento del orquestador en el diario (arranque o parada). El diario es
/// secundario: si la base no abre, queda en el registro y sigue.
fn diario(accion: &str, detalle: Option<String>) {
    let r = Store::open_at(&ea_ajustes::cargar().rutas.db).and_then(|s| {
        s.append_event(NewEvent {
            actor: ACTOR.to_string(),
            action: accion.to_string(),
            outcome: Some(EventOutcome::Ok),
            detail: detalle,
            ..Default::default()
        })
    });
    if let Err(e) = r {
        tracing::warn!(%accion, error = %e, "no se pudo anotar en el diario");
    }
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
    let config = efectivos.ajustes.orquestador;

    // Un solo orquestador por bus: sin el candado de instancia sale aquí, sin
    // latir ni tocar nada. Con él, recupera lo que se quedó a medias.
    let bus = abrir_bus()?;
    let _instancia = arrancar(&bus).map_err(anyhow::Error::msg)?;
    drop(bus);

    let intervalo_heartbeat = config.intervalo_heartbeat_s;
    // Se relee en cada latido: `agente.sandbox` es en caliente. El motivo, si
    // el aislamiento no puede arrancar, viaja en el latido para que el panel lo
    // muestre (spec 020, «sandbox visible»).
    let sandbox_info = || {
        let activado = ea_ajustes::cargar_de(&ea_ajustes::Fuentes::sistema()).ajustes.agente.sandbox;
        ea_orchestrator::sandbox::Sandbox::actual(activado).info(ea_plataforma::Sistema::actual())
    };
    // El latido va a `orquestador.json` del bus (U-07); en el diario, solo el
    // arranque y la parada.
    diario("orquestador_arrancado", serde_json::to_string(&sandbox_info()).ok());
    let latido = tokio::spawn(async move {
        loop {
            let latir = Bus::abrir_en(&ea_ajustes::cargar().rutas.estado).and_then(|b| b.latir_orquestador(Some(sandbox_info())));
            if let Err(e) = latir {
                tracing::warn!(error = %e, "no se pudo escribir el latido");
            }
            tokio::time::sleep(Duration::from_secs(intervalo_heartbeat)).await;
        }
    });

    tracing::info!("ea-orchestrator arrancado");

    // Un trabajador por ejecutor despierto (spec 019): los miembros ejecutor de
    // los equipos despiertos o, sin equipos, solo `roles.ejecutor`. Cada
    // trabajador rehace su configuración y su adaptador en cada ciclo y el
    // agente tarda minutos, así que van en hilos aparte; aquí solo se
    // reconcilia quién debe tener trabajador.
    let sondeo = Duration::from_secs(config.intervalo_sondeo_s);
    let mut trabajadores = Trabajadores::new(Contexto::sistema(|| abrir_bus().ok(), sondeo));
    let parada = ea_plataforma::parada::esperar_parada();
    tokio::pin!(parada);
    loop {
        trabajadores.reconciliar();
        tokio::select! {
            _ = &mut parada => break,
            _ = tokio::time::sleep(sondeo) => {}
        }
    }
    // SIGTERM o Ctrl-C: lo que corre se corta y se queda `ejecutando`; al
    // volver, `recuperar` lo entrega como «interrumpido».
    tracing::info!("parada pedida: se corta lo que está en marcha");
    tokio::task::spawn_blocking(move || trabajadores.detener()).await?;
    latido.abort();
    diario("orquestador_parado", None);
    tracing::info!("ea-orchestrator parado");
    Ok(())
}
