//! Superficie HTTP local de voz de `ea-orchestrator`: una sola ruta,
//! `POST /v1/voz/hablar`, en el mismo estilo que ya usa `ea-proxy` (axum
//! sobre `127.0.0.1`). Es el disparador de activación manual (§6.5 "nivel
//! bajo demanda") — el orquestador posee el dispositivo de audio (§6.3), así
//! que la CLI (`ea voz hablar`) no puede grabar por sí misma: le pide a este
//! proceso que escuche, y este responde con la transcripción y la respuesta
//! ya habladas. Esto no rompe "la interfaz nunca habla con los agentes"
//! (§12.2): el orquestador no es un agente (§3), y la llamada sigue creando
//! el mismo artefacto de tarea que cualquier otro origen (§9.1).

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use ea_store::{EventOutcome, NewEvent, NewTask, Store, TaskOrigin, TaskStatus};
use serde::Serialize;
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::proxy_client::ProxyClient;
use crate::voz::{self, MotorVoz, Reconocedor, Sintetizador};

const ACTOR: &str = "orquestador";
const AVISO_NO_ENTENDIDO: &str = "No te he entendido.";

pub struct AppState {
    pub store: Mutex<Store>,
    pub proxy: ProxyClient,
    /// `None` si los ficheros de modelo no estaban disponibles al arrancar
    /// — el resto del orquestador (sondeo de rutinas, heartbeat) sigue
    /// funcionando igual; solo esta ruta queda inservible.
    pub motor: Option<Arc<MotorVoz>>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/voz/hablar", post(hablar))
        .with_state(state)
}

#[derive(Serialize)]
struct HablarResponse {
    transcripcion: Option<String>,
    respuesta: Option<String>,
    aviso: Option<String>,
}

async fn log_event(store: &Mutex<Store>, action: &str, task_id: Option<i64>, detail: serde_json::Value) {
    let store = store.lock().await;
    let _ = store.append_event(NewEvent {
        actor: ACTOR.to_string(),
        action: action.to_string(),
        task_id,
        outcome: Some(EventOutcome::Ok),
        duration_ms: None,
        detail: Some(detail.to_string()),
    });
}

fn error_interno(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn hablar(State(state): State<Arc<AppState>>) -> Result<Json<HablarResponse>, (StatusCode, String)> {
    let motor = state.motor.clone().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "motor de voz no disponible: revisa los ficheros de modelo (ver README)".to_string(),
        )
    })?;

    log_event(&state.store, "voz_escucha_iniciada", None, json!({})).await;

    let motor_vad = motor.clone();
    let audio = tokio::task::spawn_blocking(move || voz::captura::escuchar_una_frase(&motor_vad.vad))
        .await
        .map_err(error_interno)?
        .map_err(error_interno)?;

    let Some(audio) = audio else {
        log_event(&state.store, "voz_transcripcion_descartada", None, json!({"motivo": "sin_voz"})).await;
        hablar_aviso(&motor, AVISO_NO_ENTENDIDO).await;
        return Ok(Json(HablarResponse {
            transcripcion: None,
            respuesta: None,
            aviso: Some(AVISO_NO_ENTENDIDO.to_string()),
        }));
    };

    let motor_asr = motor.clone();
    let transcripcion = tokio::task::spawn_blocking(move || motor_asr.reconocedor.transcribir(&audio))
        .await
        .map_err(error_interno)?
        .map_err(error_interno)?;
    let texto = transcripcion.texto.trim().to_string();

    // Segunda mitad de la mitigación de §6.6: la puerta de VAD ya aseguró
    // que hubo voz; esto descarta transcripciones vacías (el reconocedor
    // offline de sherpa-onnx no expone una puntuación de confianza).
    if texto.is_empty() {
        log_event(&state.store, "voz_transcripcion_descartada", None, json!({"motivo": "texto_vacio"})).await;
        hablar_aviso(&motor, AVISO_NO_ENTENDIDO).await;
        return Ok(Json(HablarResponse {
            transcripcion: None,
            respuesta: None,
            aviso: Some(AVISO_NO_ENTENDIDO.to_string()),
        }));
    }

    let tarea = {
        let mut store = state.store.lock().await;
        store
            .create_task(NewTask {
                created_by: "usuario".to_string(),
                assigned_to: "modelo-local".to_string(),
                origin: TaskOrigin::Voz,
                parent_task_id: None,
                title: format!("Orden de voz: {}", resumen(&texto)),
                request: texto.clone(),
            })
            .map_err(error_interno)?
    };
    log_event(&state.store, "voz_transcripcion", Some(tarea.id), json!({"texto": texto})).await;
    {
        let mut store = state.store.lock().await;
        store
            .update_task_status(tarea.id, TaskStatus::EnProgreso, ACTOR, None)
            .map_err(error_interno)?;
    }

    let started = Instant::now();
    let respuesta = match state.proxy.completar(&texto).await {
        Ok(texto_respuesta) => texto_respuesta,
        Err(e) => {
            let mut store = state.store.lock().await;
            let _ = store.update_task_status(tarea.id, TaskStatus::Fallida, ACTOR, Some(e.to_string()));
            drop(store);
            log_event(
                &state.store,
                "voz_respuesta",
                Some(tarea.id),
                json!({"error": e.to_string(), "duration_ms": started.elapsed().as_millis()}),
            )
            .await;
            return Err((StatusCode::BAD_GATEWAY, format!("ea-proxy no pudo responder: {e}")));
        }
    };
    log_event(
        &state.store,
        "voz_respuesta",
        Some(tarea.id),
        json!({"respuesta": respuesta, "duration_ms": started.elapsed().as_millis()}),
    )
    .await;
    {
        let mut store = state.store.lock().await;
        store
            .update_task_status(tarea.id, TaskStatus::Completada, "modelo-local", Some(respuesta.clone()))
            .map_err(error_interno)?;
    }

    let motor_tts = motor.clone();
    let texto_hablar = respuesta.clone();
    let resultado_habla = tokio::task::spawn_blocking(move || {
        motor_tts.sintetizador.hablar(&texto_hablar, &CancellationToken::new())
    })
    .await
    .map_err(error_interno)?;
    match resultado_habla {
        Ok(()) => log_event(&state.store, "voz_habla", Some(tarea.id), json!({"outcome": "ok"})).await,
        Err(e) => log_event(&state.store, "voz_habla", Some(tarea.id), json!({"outcome": "error", "error": e.to_string()})).await,
    }

    Ok(Json(HablarResponse {
        transcripcion: Some(texto),
        respuesta: Some(respuesta),
        aviso: None,
    }))
}

/// Aviso breve hablado (ej. "no te he entendido") — no crea tarea ni pasa
/// por `ea-proxy`, así que corre directo sobre el motor.
async fn hablar_aviso(motor: &Arc<MotorVoz>, texto: &str) {
    let motor = motor.clone();
    let texto = texto.to_string();
    let _ = tokio::task::spawn_blocking(move || motor.sintetizador.hablar(&texto, &CancellationToken::new())).await;
}

fn resumen(texto: &str) -> String {
    const MAX: usize = 60;
    if texto.chars().count() <= MAX {
        texto.to_string()
    } else {
        let cortado: String = texto.chars().take(MAX).collect();
        format!("{cortado}…")
    }
}
