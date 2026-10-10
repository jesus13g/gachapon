//! Tarea permanente de la palabra de activación (§6.5 "nivel permanente").
//! Solo escucha y decide; cuando se activa, entrega el control al mismo
//! turno de voz que usa la activación manual (`turno::ejecutar`), sin
//! duplicar nada de su flujo.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use sherpa_onnx::VoiceActivityDetector;
use tokio_util::sync::CancellationToken;

use crate::http::AppState;
use crate::turno::{self, Disparador, ErrorTurno};
use crate::voz::palabra::{DetectorPalabra, Escucha};
use crate::voz::{sintesis, MotorVoz};

const ESPERA_ENTRE_FALLOS: Duration = Duration::from_secs(10);
const ESPERA_TURNO_AJENO: Duration = Duration::from_millis(200);

pub async fn bucle(
    state: Arc<AppState>,
    motor: Arc<MotorVoz>,
    detector: Arc<DetectorPalabra>,
    vad: Arc<VoiceActivityDetector>,
) {
    turno::registrar(
        &state.store,
        "voz_palabra_escucha_iniciada",
        None,
        json!({"palabra": detector.palabra}),
    )
    .await;

    let mut filtro = detector.nuevo_filtro();
    let mut fallo_registrado = false;

    loop {
        // Mientras otro turno (el manual) tiene el micrófono, no se escucha.
        if state.turno_en_curso.load(Ordering::Acquire) {
            tokio::time::sleep(ESPERA_TURNO_AJENO).await;
            continue;
        }

        let (det, v, st) = (detector.clone(), vad.clone(), state.clone());
        let (resultado, devuelto) = match tokio::task::spawn_blocking(move || {
            let r = det.esperar(&v, &mut filtro, &st.turno_en_curso);
            (r, filtro)
        })
        .await
        {
            Ok(x) => x,
            Err(e) => {
                tracing::error!(error = %e, "la escucha de la palabra de activación terminó de forma anómala");
                return;
            }
        };
        filtro = devuelto;

        match resultado {
            Ok(Escucha::Interrumpida) => {}
            Ok(Escucha::Descartada(motivo)) => {
                turno::registrar(
                    &state.store,
                    "voz_palabra_descartada",
                    None,
                    json!({"palabra": detector.palabra, "motivo": motivo.como_str()}),
                )
                .await;
            }
            Ok(Escucha::Activada) => {
                fallo_registrado = false;
                turno::registrar(&state.store, "voz_palabra_detectada", None, json!({"palabra": detector.palabra})).await;
                if state.turno_en_curso.load(Ordering::Acquire) {
                    continue;
                }
                let _ = tokio::task::spawn_blocking(|| sintesis::pitido(&CancellationToken::new())).await;
                match turno::ejecutar(&state, &motor, Disparador::PalabraActivacion).await {
                    Ok(_) | Err(ErrorTurno::Ocupado) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "turno de voz por palabra de activación fallido");
                        let store = state.store.lock().await;
                        let _ = store.append_event(ea_store::NewEvent {
                            actor: turno::ACTOR.to_string(),
                            action: "voz_turno_fallido".to_string(),
                            task_id: None,
                            outcome: Some(ea_store::EventOutcome::Error),
                            duration_ms: None,
                            detail: Some(json!({"disparador": "palabra_activacion", "error": e.to_string()}).to_string()),
                        });
                    }
                }
                // Lo último que se oyó fue la respuesta del propio turno:
                // que no cuente como una activación nueva.
                filtro.marcar(Instant::now());
            }
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo escuchar la palabra de activación; reintento en 10 s");
                if !fallo_registrado {
                    fallo_registrado = true;
                    let store = state.store.lock().await;
                    let _ = store.append_event(ea_store::NewEvent {
                        actor: turno::ACTOR.to_string(),
                        action: "voz_palabra_escucha_fallida".to_string(),
                        task_id: None,
                        outcome: Some(ea_store::EventOutcome::Error),
                        duration_ms: None,
                        detail: Some(json!({"error": e.to_string()}).to_string()),
                    });
                }
                tokio::time::sleep(ESPERA_ENTRE_FALLOS).await;
            }
        }
    }
}
