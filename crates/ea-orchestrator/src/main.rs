use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use ea_orchestrator::mcp_client::McpClient;
use ea_orchestrator::proxy_client::ProxyClient;
use ea_orchestrator::voz::palabra::DetectorPalabra;
use ea_orchestrator::voz::{self, captura, MotorVoz};
use ea_orchestrator::{activacion, config, ejecutor, http, rutina};
use ea_acciones::SoloConsulta;
use ea_store::{EventOutcome, NewEvent, RunOutcome, Store, Task, TaskFilter, TaskStatus};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Actor que este proceso escribe en el diario y en las tareas que
/// procesa. Constante, a diferencia de `ea-mcp`/`ea-proxy`: este binario
/// nunca actúa "en nombre de" otro cliente.
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

    let mcp = McpClient::connect(&config.mcp_endpoint)
        .await
        .context("no se pudo conectar con ea-mcp")?;
    let proxy = ProxyClient::new(config.proxy_endpoint.clone());

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

    // Motor de voz (fase 5, §6): carga los tres modelos de sherpa-onnx una
    // sola vez. Si faltan los ficheros de modelo, esto no es fatal para el
    // resto del orquestador (sondeo de rutinas, heartbeat) — solo deja
    // inservible la ruta `/v1/voz/hablar` hasta que se resuelva.
    let modelos_voz = voz::config::load();
    let motor = match MotorVoz::cargar(&modelos_voz) {
        Ok(m) => {
            tracing::info!("motor de voz cargado");
            Some(Arc::new(m))
        }
        Err(e) => {
            tracing::warn!(error = %e, "motor de voz no disponible; /v1/voz/hablar responderá 503");
            None
        }
    };

    // Palabra de activación: desactivada por defecto (micrófono abierto de
    // forma continua). Si está activada pero no puede arrancar, el
    // orquestador sigue igual: rutinas, heartbeat y voz manual no dependen
    // de ella.
    let cfg_palabra = modelos_voz.palabra_activacion.clone();
    let mut estado_palabra = http::EstadoPalabra::Desactivada;
    let mut palabra_lista = None;
    if cfg_palabra.activada {
        estado_palabra = match &motor {
            None => http::EstadoPalabra::NoDisponible("motor de voz no disponible".to_string()),
            Some(_) => match DetectorPalabra::cargar(&cfg_palabra)
                .and_then(|d| Ok((d, captura::crear_vad(&modelos_voz)?)))
            {
                Ok((detector, vad)) => {
                    let palabra = detector.palabra.clone();
                    palabra_lista = Some((detector, vad));
                    http::EstadoPalabra::Escuchando(palabra)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "palabra de activación no disponible; la voz manual sigue funcionando");
                    http::EstadoPalabra::NoDisponible(e.to_string())
                }
            },
        };
    }

    let voz_store = Store::open().context("no se pudo abrir la base de datos para la voz")?;
    let voz_proxy = ProxyClient::new(config.proxy_endpoint.clone());
    let voz_state = Arc::new(http::AppState {
        store: Mutex::new(voz_store),
        proxy: voz_proxy,
        motor: motor.clone(),
        turno_en_curso: AtomicBool::new(false),
        palabra: std::sync::Mutex::new(estado_palabra),
    });
    if let (Some((detector, vad)), Some(motor)) = (palabra_lista, motor) {
        tokio::spawn(activacion::bucle(voz_state.clone(), motor, Arc::new(detector), Arc::new(vad)));
    }
    let voz_bind = format!("127.0.0.1:{}", config.voz_puerto);
    let voz_listener = tokio::net::TcpListener::bind(&voz_bind)
        .await
        .with_context(|| format!("no se pudo escuchar en {voz_bind}"))?;
    tokio::spawn(async move {
        tracing::info!(bind_addr = %voz_bind, "superficie de voz arrancada");
        if let Err(e) = axum::serve(voz_listener, http::router(voz_state))
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        {
            tracing::error!(error = %e, "la superficie de voz terminó con error");
        }
    });

    tracing::info!(rutinas_dir = %config.rutinas_dir.display(), "ea-orchestrator arrancado");

    loop {
        if let Err(e) = ciclo_sondeo(&config.rutinas_dir, &mcp, &proxy).await {
            tracing::error!(error = %e, "fallo en el ciclo de sondeo");
        }
        tokio::time::sleep(Duration::from_secs(config.intervalo_sondeo_s)).await;
    }
}

async fn ciclo_sondeo(rutinas_dir: &Path, mcp: &McpClient, proxy: &ProxyClient) -> anyhow::Result<()> {
    let mut store = Store::open().context("no se pudo abrir la base de datos")?;

    let pendientes = store.list_tasks(&TaskFilter {
        status: Some(TaskStatus::Pendiente),
        assigned_to: Some(ACTOR.to_string()),
        ..Default::default()
    })?;

    for tarea in pendientes {
        procesar_tarea(&mut store, rutinas_dir, &tarea, mcp, proxy).await;
    }
    Ok(())
}

const DETALLE_MAX_CHARS: usize = 4000;

/// Un paso `accion` puede devolver un payload grande — notablemente,
/// `ea_listar_diario` leyéndose a sí mismo: sin límite, el `detalle` de
/// cada `rutina_paso` crecería sin cota de una ejecución a la siguiente,
/// porque el diario es append-only (§8.2) y cada ejecución vuelve a leer
/// los `rutina_paso` de la anterior. Se acota solo lo que se persiste; lo
/// que ve el paso siguiente dentro de la misma ejecución no pasa por aquí
/// (`salida_previa` en `ejecutor.rs`).
fn acotar_detalle(valor: &serde_json::Value) -> String {
    let texto = valor.to_string();
    if texto.len() <= DETALLE_MAX_CHARS {
        return texto;
    }
    let cortado: String = texto.chars().take(DETALLE_MAX_CHARS).collect();
    format!("{cortado}... [truncado, {} bytes originales]", texto.len())
}

async fn procesar_tarea(
    store: &mut Store,
    rutinas_dir: &Path,
    tarea: &Task,
    mcp: &McpClient,
    proxy: &ProxyClient,
) {
    let _ = store.update_task_status(tarea.id, TaskStatus::EnProgreso, ACTOR, None);

    let slug = match serde_json::from_str::<serde_json::Value>(&tarea.request)
        .ok()
        .and_then(|v| v["rutina"].as_str().map(str::to_string))
    {
        Some(s) => s,
        None => {
            let _ = store.update_task_status(
                tarea.id,
                TaskStatus::Fallida,
                ACTOR,
                Some("tarea sin campo 'rutina' en su solicitud".to_string()),
            );
            return;
        }
    };

    let rutina = match rutina::cargar(rutinas_dir, &slug) {
        Ok(r) => r,
        Err(e) => {
            let _ = store.update_task_status(
                tarea.id,
                TaskStatus::Fallida,
                ACTOR,
                Some(format!("no se pudo cargar la rutina '{slug}': {e}")),
            );
            let _ = store.append_event(NewEvent {
                actor: ACTOR.to_string(),
                action: "rutina_carga_fallida".to_string(),
                task_id: Some(tarea.id),
                outcome: Some(EventOutcome::Error),
                duration_ms: None,
                detail: Some(serde_json::json!({ "rutina": slug, "error": e.to_string() }).to_string()),
            });
            return;
        }
    };

    // §10.2: la ventana real transcurrida, no la nominal — desde la última
    // ejecución con éxito, o desde `ventana_inicial_dias` si es la primera.
    let ultima_ejecucion = match store.last_successful_run(&slug) {
        Ok(Some(run)) => run.started_at,
        _ => OffsetDateTime::now_utc() - time::Duration::days(rutina.ventana_inicial_dias),
    };
    let ultima_ejecucion_str = ultima_ejecucion
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| ultima_ejecucion.to_string());

    let run = match store.start_routine_run(&slug, Some(tarea.id)) {
        Ok(r) => r,
        Err(e) => {
            let _ = store.update_task_status(
                tarea.id,
                TaskStatus::Fallida,
                ACTOR,
                Some(format!("no se pudo abrir la ejecución: {e}")),
            );
            return;
        }
    };

    let resultado = ejecutor::ejecutar(&rutina, &ultima_ejecucion_str, mcp, proxy).await;

    for paso in &resultado.pasos {
        let _ = store.append_event(NewEvent {
            actor: ACTOR.to_string(),
            action: "rutina_paso".to_string(),
            task_id: Some(tarea.id),
            outcome: Some(EventOutcome::Ok),
            duration_ms: None,
            detail: Some(acotar_detalle(&serde_json::json!({
                "rutina": slug, "paso": paso.id, "tipo": paso.tipo, "detalle": paso.detalle,
            }))),
        });
    }

    // Lo desatendido solo propone: `SoloConsulta` no implementa `Sistema`, así
    // que desde aquí es imposible mutar el sistema, sea cual sea la rutina.
    let consulta = SoloConsulta {
        rutinas_dir: rutinas_dir.to_path_buf(),
        boveda: ea_acciones::Boveda::configurada(),
    };
    for pr in &resultado.propuestas {
        let mut descripcion = pr.descripcion.clone();
        let creada = match &pr.accion {
            Some(a) => match async {
                let hechos = ea_acciones::observar(&consulta, &a.herramienta, &a.argumentos).await?;
                ea_acciones::proponer(
                    store, &consulta, &hechos, ACTOR, Some(run.id), Some(&pr.descripcion),
                    &a.herramienta, &a.argumentos,
                )
            }
            .await
            {
                Ok((p, _)) => Ok(p),
                Err(e) => {
                    // La acción ya no aplica (o no se pudo simular): la
                    // propuesta se conserva como texto, con el motivo.
                    descripcion = format!("{} [acción '{}' no propuesta: {e}]", pr.descripcion, a.herramienta);
                    store.create_propuesta(run.id, &descripcion)
                }
            },
            None => store.create_propuesta(run.id, &pr.descripcion),
        };
        if let Ok(propuesta) = creada {
            let _ = store.append_event(NewEvent {
                actor: ACTOR.to_string(),
                action: "rutina_propuesta".to_string(),
                task_id: Some(tarea.id),
                outcome: Some(EventOutcome::Ok),
                duration_ms: None,
                detail: Some(
                    serde_json::json!({
                        "rutina": slug, "propuesta_id": propuesta.id, "descripcion": descripcion,
                        "accion": propuesta.accion.as_ref().map(|a| serde_json::json!({
                            "herramienta": a.herramienta, "argumentos": a.argumentos, "riesgo": a.riesgo,
                        })),
                    })
                    .to_string(),
                ),
            });
        }
    }

    let _ = store.finish_routine_run(
        run.id,
        resultado.outcome,
        resultado.pasos.len() as i64,
        resultado.llamadas_modelo,
        resultado.motivo_fallo.clone(),
    );

    match resultado.outcome {
        RunOutcome::Ok => {
            let resumen = format!(
                "{} paso(s), {} propuesta(s)",
                resultado.pasos.len(),
                resultado.propuestas.len()
            );
            let _ = store.update_task_status(tarea.id, TaskStatus::Completada, ACTOR, Some(resumen));
        }
        RunOutcome::Fallida => {
            let _ = store.update_task_status(tarea.id, TaskStatus::Fallida, ACTOR, resultado.motivo_fallo);
        }
    }
}
