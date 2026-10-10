//! Frontera de voz del ecosistema (§6.2 de la especificación): contratos
//! diferenciados de reconocimiento y síntesis, para que una implementación
//! fuera de proceso sea sustituible sin tocar la lógica del asistente. Todo
//! lo que cruza esta frontera es audio en memoria (muestras `f32` a 16kHz
//! mono, el formato que usa `sherpa-onnx`) o texto — nunca rutas de fichero.
//!
//! Las llamadas de `sherpa-onnx` son FFI síncrona (no `async`); el llamador
//! (`http.rs`) las ejecuta con `tokio::task::spawn_blocking` para no
//! bloquear el runtime.

pub mod captura;
pub mod config;
pub mod palabra;
pub mod reconocimiento;
pub mod sintesis;

use tokio_util::sync::CancellationToken;

/// Resultado de una transcripción. Sin campo de confianza numérico: la API
/// de reconocimiento offline de `sherpa-onnx` no expone una puntuación de
/// confianza para modelos Whisper. La mitigación real del riesgo de §6.6
/// (texto inventado en silencios) es la puerta de VAD antes de transcribir
/// (`captura::escuchar_una_frase` no produce audio si no hubo voz) más
/// descartar texto vacío o en blanco después de transcribir — ver
/// `http.rs`.
pub struct Transcripcion {
    pub texto: String,
}

pub trait Reconocedor: Send + Sync {
    fn transcribir(&self, audio: &[f32]) -> anyhow::Result<Transcripcion>;
}

pub trait Sintetizador: Send + Sync {
    /// `cancelacion` es el punto de anclaje de la interrupción que exige
    /// §6.2 de la especificación ("el contrato de síntesis debe incluir
    /// cancelación desde el primer día, aunque la primera versión no la
    /// implemente") — nada la dispara todavía en esta fase; el motor de
    /// reproducción revisa `cancelacion.is_cancelled()` entre bloques de
    /// audio para que un futuro disparador (interrupción por voz del
    /// usuario) solo tenga que llamar a `cancel()`.
    fn hablar(&self, texto: &str, cancelacion: &CancellationToken) -> anyhow::Result<()>;
}

/// Agrupa las tres piezas cargadas de `sherpa-onnx` (VAD, reconocedor,
/// sintetizador) — una sola carga de modelos al arrancar el orquestador,
/// compartida entre invocaciones de `/v1/voz/hablar` vía `Arc`.
pub struct MotorVoz {
    pub vad: sherpa_onnx::VoiceActivityDetector,
    pub reconocedor: reconocimiento::SherpaReconocedor,
    pub sintetizador: sintesis::SherpaSintetizador,
}

impl MotorVoz {
    pub fn cargar(modelos: &config::VozConfig) -> anyhow::Result<Self> {
        modelos.verificar()?;
        Ok(Self {
            vad: captura::crear_vad(modelos)?,
            reconocedor: reconocimiento::SherpaReconocedor::cargar(modelos)?,
            sintetizador: sintesis::SherpaSintetizador::cargar(modelos)?,
        })
    }
}
