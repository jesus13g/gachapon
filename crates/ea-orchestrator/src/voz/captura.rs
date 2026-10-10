//! Captura de audio (`cpal`) gateada por VAD (Silero, vía `sherpa-onnx`):
//! produce el buffer de una frase completa, o `None` si no hubo voz dentro
//! del tiempo de espera — nunca una grabación sin límite (§6.6 de la
//! especificación: "detección de actividad vocal previa: sin voz no hay
//! inferencia"). El proceso que posee el dispositivo de captura es el que
//! decide cuándo hay voz (§6.3) — por eso esto vive en `ea-orchestrator`,
//! no en la CLI.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::config::VozConfig;
use super::reconocimiento::SAMPLE_RATE;

/// Tope duro de duración de una frase: el VAD la corta aquí aunque el
/// usuario siga hablando.
const DURACION_MAXIMA_FRASE_S: f32 = 15.0;
/// Tiempo total de espera sin ninguna voz detectada antes de desistir.
const ESPERA_MAXIMA_SIN_VOZ: Duration = Duration::from_secs(20);

pub fn crear_vad(modelos: &VozConfig) -> anyhow::Result<VoiceActivityDetector> {
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(modelos.vad_model.to_string_lossy().into_owned()),
            threshold: 0.5,
            min_silence_duration: 0.5,
            min_speech_duration: 0.25,
            window_size: 512,
            max_speech_duration: DURACION_MAXIMA_FRASE_S,
        },
        sample_rate: SAMPLE_RATE,
        provider: Some("cpu".to_string()),
        ..Default::default()
    };
    VoiceActivityDetector::create(&config, 30.0)
        .ok_or_else(|| anyhow::anyhow!("sherpa-onnx no pudo cargar el VAD"))
}

/// Graba desde el dispositivo de entrada por defecto hasta que el VAD
/// finaliza un segmento de voz (silencio tras haber habido voz, o el tope
/// duro de duración) o hasta que pasa `ESPERA_MAXIMA_SIN_VOZ` sin que
/// empiece a haber voz. `Ok(None)` significa "no hubo voz que transcribir"
/// — nunca se inventa una transcripción sobre silencio.
pub fn escuchar_una_frase(vad: &VoiceActivityDetector) -> anyhow::Result<Option<Vec<f32>>> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow::anyhow!("no hay dispositivo de audio de entrada"))?;

    let stream_config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE as u32),
        buffer_size: cpal::BufferSize::Default,
    };

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[f32], _| {
            let _ = tx.send(data.to_vec());
        },
        |err| tracing::warn!(error = %err, "error en la captura de audio"),
        None,
    )?;
    stream.play()?;

    vad.reset();
    let inicio = Instant::now();

    let resultado = loop {
        if inicio.elapsed() > ESPERA_MAXIMA_SIN_VOZ {
            break None;
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => vad.accept_waveform(&chunk),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break None,
        }

        if !vad.is_empty() {
            let segmento = vad.front().map(|s| s.samples().to_vec());
            vad.pop();
            break segmento;
        }
    };

    drop(stream);
    Ok(resultado)
}
