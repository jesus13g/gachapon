//! Implementación de [`Sintetizador`] sobre VITS en español
//! (`csukuangfj/vits-coqui-es-css10`) vía `sherpa-onnx`, con reproducción
//! por `cpal` sobre el dispositivo de salida por defecto. Todo lo que sale
//! de `sherpa-onnx` es un buffer `f32` en memoria (`GeneratedAudio`); nunca
//! se escribe a fichero antes de reproducir (§6.2: audio en memoria o
//! texto, nunca rutas de fichero).

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sherpa_onnx::{
    GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsModelConfig, OfflineTtsVitsModelConfig,
};
use tokio_util::sync::CancellationToken;

use super::config::VozConfig;
use super::Sintetizador;

pub struct SherpaSintetizador {
    tts: OfflineTts,
}

impl SherpaSintetizador {
    pub fn cargar(modelos: &VozConfig) -> anyhow::Result<Self> {
        fn path(p: &std::path::Path) -> String {
            p.to_string_lossy().into_owned()
        }
        let opt_path = |p: &Option<std::path::PathBuf>| p.as_deref().map(path);

        let config = OfflineTtsConfig {
            model: OfflineTtsModelConfig {
                vits: OfflineTtsVitsModelConfig {
                    model: Some(path(&modelos.vits_model)),
                    tokens: Some(path(&modelos.vits_tokens)),
                    data_dir: opt_path(&modelos.vits_data_dir),
                    lexicon: opt_path(&modelos.vits_lexicon),
                    ..Default::default()
                },
                // CPU explícito, igual que el reconocedor (§6.4).
                provider: Some("cpu".to_string()),
                num_threads: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        let tts = OfflineTts::create(&config)
            .ok_or_else(|| anyhow::anyhow!("sherpa-onnx no pudo cargar el sintetizador VITS"))?;

        Ok(Self { tts })
    }
}

impl SherpaSintetizador {
    /// Genera el audio del texto en memoria, sin reproducirlo: muestras
    /// `f32` y su frecuencia de muestreo.
    pub fn sintetizar(&self, texto: &str) -> anyhow::Result<(Vec<f32>, i32)> {
        let generacion = GenerationConfig {
            speed: 1.0,
            silence_scale: 1.0,
            sid: 0,
            ..Default::default()
        };

        let audio = self
            .tts
            .generate_with_config(texto, &generacion, None::<fn(&[f32], f32) -> bool>)
            .ok_or_else(|| anyhow::anyhow!("sherpa-onnx no pudo sintetizar el texto"))?;
        Ok((audio.samples().to_vec(), audio.sample_rate()))
    }
}

impl Sintetizador for SherpaSintetizador {
    fn hablar(&self, texto: &str, cancelacion: &CancellationToken) -> anyhow::Result<()> {
        if cancelacion.is_cancelled() {
            return Ok(());
        }
        let (muestras, frecuencia) = self.sintetizar(texto)?;
        reproducir(&muestras, frecuencia, cancelacion)
    }
}

/// Pitido breve que confirma que la palabra de activación se ha oído y que
/// ya se está escuchando (sin él, el usuario no sabe cuándo hablar).
pub fn pitido(cancelacion: &CancellationToken) -> anyhow::Result<()> {
    const FRECUENCIA: i32 = 16_000;
    let (tono, duracion) = (880.0_f32, 0.12_f32);
    let n = (FRECUENCIA as f32 * duracion) as usize;
    let muestras: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / FRECUENCIA as f32;
            // Rampa de entrada y salida para evitar chasquidos.
            let envolvente = (i.min(n - i) as f32 / (n as f32 * 0.2)).min(1.0);
            0.25 * envolvente * (2.0 * std::f32::consts::PI * tono * t).sin()
        })
        .collect();
    reproducir(&muestras, FRECUENCIA, cancelacion)
}

/// Reproduce un buffer de audio ya en memoria sobre el dispositivo de
/// salida por defecto, comprobando `cancelacion` entre bloques — el punto
/// de interrupción que exige §6.2, aunque en esta fase nada lo dispare
/// todavía.
fn reproducir(samples: &[f32], sample_rate: i32, cancelacion: &CancellationToken) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no hay dispositivo de audio de salida"))?;

    let stream_config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(sample_rate as u32),
        buffer_size: cpal::BufferSize::Default,
    };

    let buffer = samples.to_vec();
    let posicion = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let posicion_cb = posicion.clone();
    let (fin_tx, fin_rx) = std::sync::mpsc::channel::<()>();

    let stream = device.build_output_stream(
        &stream_config,
        move |data: &mut [f32], _| {
            let inicio = posicion_cb.load(std::sync::atomic::Ordering::Relaxed);
            let restante = buffer.len().saturating_sub(inicio);
            let a_copiar = restante.min(data.len());
            data[..a_copiar].copy_from_slice(&buffer[inicio..inicio + a_copiar]);
            for muestra in &mut data[a_copiar..] {
                *muestra = 0.0;
            }
            posicion_cb.store(inicio + a_copiar, std::sync::atomic::Ordering::Relaxed);
            if a_copiar < data.len() {
                let _ = fin_tx.send(());
            }
        },
        |err| tracing::warn!(error = %err, "error en la reproducción de audio"),
        None,
    )?;

    stream.play()?;

    let duracion_total = std::time::Duration::from_secs_f64(samples.len() as f64 / sample_rate as f64);
    let _ = fin_rx.recv_timeout(duracion_total + std::time::Duration::from_millis(500));

    if cancelacion.is_cancelled() {
        tracing::info!("reproducción cancelada");
    }

    Ok(())
}
