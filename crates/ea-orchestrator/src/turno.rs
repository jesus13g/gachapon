//! Un turno de voz completo: escuchar → transcribir → tarea `origin=voz` →
//! `ea-proxy` → hablar la respuesta. Es el único camino: lo recorren igual
//! la activación manual (`POST /v1/voz/hablar`, `ea voz hablar`) y la
//! palabra de activación (`palabra.rs` + la tarea de `main.rs`); solo cambia
//! el [`Disparador`] que queda anotado en el diario.

use std::sync::Arc;
use std::time::Instant;

use ea_store::{EventOutcome, NewEvent, NewTask, TaskOrigin, TaskStatus};
use serde::Serialize;
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::http::AppState;
use crate::voz::palabra::GuardaTurno;
use crate::voz::{self, MotorVoz, Reconocedor, Sintetizador};

pub const ACTOR: &str = "orquestador";
const AVISO_NO_ENTENDIDO: &str = "No te he entendido.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disparador {
    Manual,
    PalabraActivacion,
}

impl Disparador {
    pub fn como_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::PalabraActivacion => "palabra_activacion",
        }
    }
}

#[derive(Serialize)]
pub struct HablarResponse {
    pub transcripcion: Option<String>,
    pub respuesta: Option<String>,
    pub aviso: Option<String>,
}

#[derive(Debug)]
pub enum ErrorTurno {
    /// Ya hay un turno de voz en curso: el micrófono es de uno solo.
    Ocupado,
    /// `ea-proxy` no pudo responder.
    Proxy(String),
    Interno(String),
}

impl std::fmt::Display for ErrorTurno {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ocupado => write!(f, "ya hay un turno de voz en curso"),
            Self::Proxy(e) => write!(f, "ea-proxy no pudo responder: {e}"),
            Self::Interno(e) => write!(f, "{e}"),
        }
    }
}

fn interno(e: impl std::fmt::Display) -> ErrorTurno {
    ErrorTurno::Interno(e.to_string())
}

pub async fn registrar(store: &Mutex<ea_store::Store>, action: &str, task_id: Option<i64>, detail: serde_json::Value) {
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

/// Ejecuta un turno de voz. Toma la guarda de turno único: si otro turno
/// tiene el micrófono devuelve [`ErrorTurno::Ocupado`] sin tocar nada.
pub async fn ejecutar(state: &Arc<AppState>, motor: &Arc<MotorVoz>, disparador: Disparador) -> Result<HablarResponse, ErrorTurno> {
    let _guarda = GuardaTurno::intentar(&state.turno_en_curso).ok_or(ErrorTurno::Ocupado)?;

    registrar(&state.store, "voz_escucha_iniciada", None, json!({"disparador": disparador.como_str()})).await;

    let motor_vad = motor.clone();
    let audio = tokio::task::spawn_blocking(move || voz::captura::escuchar_una_frase(&motor_vad.vad))
        .await
        .map_err(interno)?
        .map_err(interno)?;

    let Some(audio) = audio else {
        return descartar(state, motor, "sin_voz").await;
    };

    let motor_asr = motor.clone();
    let transcripcion = tokio::task::spawn_blocking(move || motor_asr.reconocedor.transcribir(&audio))
        .await
        .map_err(interno)?
        .map_err(interno)?;
    let texto = transcripcion.texto.trim().to_string();

    // Segunda mitad de la mitigación de §6.6: la puerta de VAD ya aseguró
    // que hubo voz; esto descarta transcripciones vacías (el reconocedor
    // offline de sherpa-onnx no expone una puntuación de confianza).
    if texto.is_empty() {
        return descartar(state, motor, "texto_vacio").await;
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
            .map_err(interno)?
    };
    registrar(
        &state.store,
        "voz_transcripcion",
        Some(tarea.id),
        json!({"texto": texto, "disparador": disparador.como_str()}),
    )
    .await;
    {
        let mut store = state.store.lock().await;
        store
            .update_task_status(tarea.id, TaskStatus::EnProgreso, ACTOR, None)
            .map_err(interno)?;
    }

    let started = Instant::now();
    let respuesta = match state.proxy.completar(&texto).await {
        Ok(texto_respuesta) => texto_respuesta,
        Err(e) => {
            let mut store = state.store.lock().await;
            let _ = store.update_task_status(tarea.id, TaskStatus::Fallida, ACTOR, Some(e.to_string()));
            drop(store);
            registrar(
                &state.store,
                "voz_respuesta",
                Some(tarea.id),
                json!({"error": e.to_string(), "duration_ms": started.elapsed().as_millis()}),
            )
            .await;
            return Err(ErrorTurno::Proxy(e.to_string()));
        }
    };
    registrar(
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
            .map_err(interno)?;
    }

    let motor_tts = motor.clone();
    let texto_hablar = respuesta.clone();
    let resultado_habla = tokio::task::spawn_blocking(move || {
        motor_tts.sintetizador.hablar(&texto_hablar, &CancellationToken::new())
    })
    .await
    .map_err(interno)?;
    match resultado_habla {
        Ok(()) => registrar(&state.store, "voz_habla", Some(tarea.id), json!({"outcome": "ok"})).await,
        Err(e) => {
            registrar(&state.store, "voz_habla", Some(tarea.id), json!({"outcome": "error", "error": e.to_string()})).await
        }
    }

    Ok(HablarResponse { transcripcion: Some(texto), respuesta: Some(respuesta), aviso: None })
}

/// Sin voz o sin texto: no se crea tarea, se deja constancia y se avisa.
async fn descartar(state: &Arc<AppState>, motor: &Arc<MotorVoz>, motivo: &str) -> Result<HablarResponse, ErrorTurno> {
    registrar(&state.store, "voz_transcripcion_descartada", None, json!({"motivo": motivo})).await;
    hablar_aviso(motor, AVISO_NO_ENTENDIDO).await;
    Ok(HablarResponse { transcripcion: None, respuesta: None, aviso: Some(AVISO_NO_ENTENDIDO.to_string()) })
}

/// Aviso breve hablado — no crea tarea ni pasa por `ea-proxy`.
async fn hablar_aviso(motor: &Arc<MotorVoz>, texto: &str) {
    let motor = motor.clone();
    let texto = texto.to_string();
    let _ = tokio::task::spawn_blocking(move || motor.sintetizador.hablar(&texto, &CancellationToken::new())).await;
}

pub fn resumen(texto: &str) -> String {
    const MAX: usize = 60;
    if texto.chars().count() <= MAX {
        texto.to_string()
    } else {
        let cortado: String = texto.chars().take(MAX).collect();
        format!("{cortado}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumen_corta_con_puntos_suspensivos() {
        assert_eq!(resumen("hola"), "hola");
        let largo = "á".repeat(80);
        let r = resumen(&largo);
        assert_eq!(r.chars().count(), 61);
        assert!(r.ends_with('…'));
    }

    #[test]
    fn disparadores_tienen_nombre_estable_en_el_diario() {
        assert_eq!(Disparador::Manual.como_str(), "manual");
        assert_eq!(Disparador::PalabraActivacion.como_str(), "palabra_activacion");
    }
}
