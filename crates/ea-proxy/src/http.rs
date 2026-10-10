//! Endpoint HTTP expuesto por `ea-proxy`: `POST /v1/chat/completions` con
//! forma de chat completion de OpenAI (§5.1: "endpoint único y estable"),
//! más los campos de trazabilidad que este proxy añade
//! (`herramienta_usada`, `argumentos_usados`). El consumidor no declara
//! `tools`: el proxy ya sabe qué catálogo reducido aplicar.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use ea_store::Store;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::Config;
use crate::decide;
use crate::llama_client::{ChatMessage, LlamaClient};
use crate::mcp_client::McpClient;

pub struct AppState {
    pub llama: LlamaClient,
    pub mcp: McpClient,
    pub store: Mutex<Store>,
    pub config: Config,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/rama", post(rama))
        .route("/health", get(health))
        .with_state(state)
}

#[derive(Deserialize)]
struct ChatRequest {
    messages: Vec<ChatMessage>,
    #[serde(default)]
    perfil: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    herramienta_usada: Option<String>,
    argumentos_usados: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Choice {
    message: Message,
}

#[derive(Serialize)]
struct Message {
    role: &'static str,
    content: String,
}

async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    let perfil_nombre = req.perfil.as_deref().unwrap_or("rapido");
    let profile = state.config.profile(perfil_nombre);

    let decision = decide::decidir(
        &state.llama,
        &state.mcp,
        &state.store,
        &req.messages,
        profile,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ChatResponse {
        choices: vec![Choice {
            message: Message {
                role: "assistant",
                content: decision.content,
            },
        }],
        herramienta_usada: decision.herramienta_usada,
        argumentos_usados: decision.argumentos_usados,
    }))
}

/// Petición de un punto de decisión de rutina (§9.2 de la especificación):
/// un contexto en lenguaje natural y un conjunto cerrado de ramas entre las
/// que el modelo local debe elegir exactamente una. A diferencia de
/// `/v1/chat/completions`, el conjunto de opciones no es el catálogo fijo
/// de herramientas sino el que declara cada paso de la rutina.
#[derive(Deserialize)]
struct RamaRequest {
    contexto: String,
    opciones: Vec<String>,
}

#[derive(Serialize)]
struct RamaResponse {
    rama: String,
}

async fn rama(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RamaRequest>,
) -> Result<Json<RamaResponse>, (StatusCode, String)> {
    if req.opciones.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "opciones no puede estar vacío".to_string(),
        ));
    }

    let elegida = decide::elegir_rama(&state.llama, &state.store, &req.contexto, &req.opciones)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(RamaResponse { rama: elegida }))
}

async fn health() -> &'static str {
    "ok"
}
