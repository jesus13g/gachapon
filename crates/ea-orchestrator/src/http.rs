//! Superficie HTTP local de voz de `ea-orchestrator`, en el mismo estilo que
//! `ea-proxy` (axum sobre `127.0.0.1`): `POST /v1/voz/hablar` (activación
//! manual, §6.5 "nivel bajo demanda") y `GET /v1/voz/estado`. El orquestador
//! posee el dispositivo de audio (§6.3), así que la CLI (`ea voz hablar`) no
//! puede grabar por sí misma: le pide a este proceso que escuche, y este
//! responde con la transcripción y la respuesta ya habladas. Esto no rompe
//! "la interfaz nunca habla con los agentes" (§12.2): el orquestador no es
//! un agente (§3), y la llamada sigue creando el mismo artefacto de tarea que
//! cualquier otro origen (§9.1). La lógica del turno está en `turno.rs`,
//! compartida con la palabra de activación.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use ea_store::Store;
use serde_json::json;
use tokio::sync::Mutex;

use crate::proxy_client::ProxyClient;
use crate::turno::{self, Disparador, ErrorTurno, HablarResponse};
use crate::voz::MotorVoz;

/// Situación de la palabra de activación, para `GET /v1/voz/estado`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EstadoPalabra {
    Desactivada,
    /// Activada en la configuración pero sin poder arrancar (modelo
    /// ausente, sin motor de voz…): el motivo.
    NoDisponible(String),
    Escuchando(String),
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub proxy: ProxyClient,
    /// `None` si los ficheros de modelo no estaban disponibles al arrancar
    /// — el resto del orquestador (sondeo de rutinas, heartbeat) sigue
    /// funcionando igual; solo esta ruta queda inservible.
    pub motor: Option<Arc<MotorVoz>>,
    /// Hay un turno de voz con el micrófono. La palabra de activación deja de
    /// escuchar mientras vale `true`.
    pub turno_en_curso: AtomicBool,
    pub palabra: std::sync::Mutex<EstadoPalabra>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/voz/hablar", post(hablar))
        .route("/v1/voz/estado", get(estado))
        .with_state(state)
}

fn a_http(e: ErrorTurno) -> (StatusCode, String) {
    let codigo = match e {
        ErrorTurno::Ocupado => StatusCode::CONFLICT,
        ErrorTurno::Proxy(_) => StatusCode::BAD_GATEWAY,
        ErrorTurno::Interno(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (codigo, e.to_string())
}

async fn hablar(State(state): State<Arc<AppState>>) -> Result<Json<HablarResponse>, (StatusCode, String)> {
    let motor = state.motor.clone().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "motor de voz no disponible: revisa los ficheros de modelo (ver README)".to_string(),
        )
    })?;
    turno::ejecutar(&state, &motor, Disparador::Manual)
        .await
        .map(Json)
        .map_err(a_http)
}

async fn estado(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let palabra = match &*state.palabra.lock().unwrap() {
        EstadoPalabra::Desactivada => json!({"estado": "desactivada"}),
        EstadoPalabra::NoDisponible(motivo) => json!({"estado": "no_disponible", "motivo": motivo}),
        EstadoPalabra::Escuchando(p) => json!({"estado": "escuchando", "palabra": p}),
    };
    Json(json!({
        "motor": state.motor.is_some(),
        "palabra_activacion": palabra,
    }))
}
