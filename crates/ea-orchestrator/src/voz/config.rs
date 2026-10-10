//! Rutas a los ficheros de modelo de voz. Se descargan a mano (mismo patrón
//! que el modelo de `llama-server`: sin script de instalación automatizado
//! en esta fase — ver README) bajo el directorio de datos XDG existente,
//! nunca vendorizados en el repositorio. Los nombres de fichero exactos
//! dependen de qué build en concreto se descargue de cada modelo; si no
//! coinciden con los defaults de aquí, se sobreescriben en `voz.toml`
//! (mismo mecanismo que `orchestrator.toml`).

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ModelosConfig {
    /// Whisper-small (offline, multilingüe): encoder, decoder y tokens.
    pub whisper_encoder: PathBuf,
    pub whisper_decoder: PathBuf,
    pub whisper_tokens: PathBuf,
    /// VITS en español (`csukuangfj/vits-coqui-es-css10`): modelo, tokens y,
    /// si el build los trae, lexicon/directorio de datos de espeak-ng.
    pub vits_model: PathBuf,
    pub vits_tokens: PathBuf,
    pub vits_data_dir: Option<PathBuf>,
    pub vits_lexicon: Option<PathBuf>,
    /// Silero VAD.
    pub vad_model: PathBuf,
}

fn voz_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
        .map(|dirs| dirs.data_dir().join("voz"))
        .unwrap_or_else(|| PathBuf::from("voz"))
}

impl Default for ModelosConfig {
    fn default() -> Self {
        let dir = voz_dir();
        Self {
            whisper_encoder: dir.join("whisper-small-encoder.onnx"),
            whisper_decoder: dir.join("whisper-small-decoder.onnx"),
            whisper_tokens: dir.join("whisper-small-tokens.txt"),
            vits_model: dir.join("vits-coqui-es-css10.onnx"),
            vits_tokens: dir.join("vits-coqui-es-css10-tokens.txt"),
            vits_data_dir: None,
            vits_lexicon: None,
            vad_model: dir.join("silero_vad.onnx"),
        }
    }
}

impl ModelosConfig {
    /// Rutas obligatorias que deben existir para poder arrancar el motor de
    /// voz. `vits_data_dir`/`vits_lexicon` son opcionales porque no todos
    /// los builds de VITS los traen.
    fn rutas_obligatorias(&self) -> [(&'static str, &PathBuf); 6] {
        [
            ("whisper_encoder", &self.whisper_encoder),
            ("whisper_decoder", &self.whisper_decoder),
            ("whisper_tokens", &self.whisper_tokens),
            ("vits_model", &self.vits_model),
            ("vits_tokens", &self.vits_tokens),
            ("vad_model", &self.vad_model),
        ]
    }

    /// Comprueba que los ficheros de modelo existen antes de intentar
    /// cargarlos — un error de `sherpa-onnx` por fichero ausente no siempre
    /// trae un mensaje útil; esto falla pronto y con contexto claro.
    pub fn verificar(&self) -> anyhow::Result<()> {
        for (nombre, ruta) in self.rutas_obligatorias() {
            anyhow::ensure!(
                ruta.is_file(),
                "falta el fichero de modelo '{nombre}' en {} (descárgalo según las \
                 instrucciones del README y ajusta la ruta en voz.toml si el nombre \
                 no coincide)",
                ruta.display()
            );
        }
        Ok(())
    }
}

fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
        .map(|dirs| dirs.config_dir().join("voz.toml"))
}

/// Carga `voz.toml` si existe y es válido; si no existe o falla el parseo,
/// cae en los defaults documentados arriba (no es un error fatal: el resto
/// del orquestador —sondeo de rutinas, heartbeat— debe seguir arrancando
/// aunque la voz no esté configurada todavía).
pub fn load() -> ModelosConfig {
    let Some(path) = config_path() else {
        return ModelosConfig::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => {
                tracing::info!(path = %path.display(), "configuración de voz cargada");
                config
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "voz.toml inválido, usando defaults");
                ModelosConfig::default()
            }
        },
        Err(_) => ModelosConfig::default(),
    }
}
