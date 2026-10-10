//! Implementación de [`Reconocedor`] sobre Whisper-small multilingüe vía
//! `sherpa-onnx` (§6.2 de la especificación: única implementación
//! concreta por ahora, detrás del contrato). Flujo "grabar y luego
//! transcribir" (no streaming) — encaja con activación manual, donde no
//! hace falta transcribir mientras el usuario sigue hablando.

use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig,
};

use super::config::VozConfig;
use super::{Reconocedor, Transcripcion};

/// Frecuencia de muestreo que espera `sherpa-onnx` para Whisper y para el
/// VAD de Silero — toda la captura (`captura.rs`) produce audio ya en este
/// formato.
pub const SAMPLE_RATE: i32 = 16_000;

pub struct SherpaReconocedor {
    recognizer: OfflineRecognizer,
}

impl SherpaReconocedor {
    pub fn cargar(modelos: &VozConfig) -> anyhow::Result<Self> {
        let path = |p: &std::path::Path| p.to_string_lossy().into_owned();

        let config = OfflineRecognizerConfig {
            model_config: OfflineModelConfig {
                whisper: OfflineWhisperModelConfig {
                    encoder: Some(path(&modelos.whisper_encoder)),
                    decoder: Some(path(&modelos.whisper_decoder)),
                    language: Some("es".to_string()),
                    task: Some("transcribe".to_string()),
                    ..Default::default()
                },
                tokens: Some(path(&modelos.whisper_tokens)),
                // Voz corre en CPU (§6.4): la GPU queda reservada al modelo
                // de lenguaje. `provider` en `None`/por defecto ya es CPU;
                // se deja explícito para que quede claro que es intencional,
                // no un olvido.
                provider: Some("cpu".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| anyhow::anyhow!("sherpa-onnx no pudo cargar el reconocedor Whisper"))?;

        Ok(Self { recognizer })
    }
}

impl Reconocedor for SherpaReconocedor {
    fn transcribir(&self, audio: &[f32]) -> anyhow::Result<Transcripcion> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, audio);
        self.recognizer.decode(&stream);
        let resultado = stream
            .get_result()
            .ok_or_else(|| anyhow::anyhow!("el reconocedor no produjo resultado"))?;
        Ok(Transcripcion { texto: resultado.text })
    }
}
