//! Rutas a los ficheros de modelo de voz. Se descargan a mano (mismo patrón
//! que el modelo de `llama-server`: sin script de instalación automatizado
//! en esta fase — ver README) bajo el directorio de datos XDG existente,
//! nunca vendorizados en el repositorio. Los nombres de fichero exactos
//! corresponden a los ficheros reales de cada modelo (ver README); si se usa
//! otro build, se sobreescriben en `voz.toml` (mismo mecanismo que
//! `orchestrator.toml`).

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct VozConfig {
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
    /// Palabra de activación (tabla `[palabra_activacion]` de `voz.toml`).
    /// Desactivada por defecto: implica el micrófono abierto de forma
    /// continua, así que el usuario tiene que pedirla a conciencia.
    pub palabra_activacion: PalabraActivacionConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct PalabraActivacionConfig {
    pub activada: bool,
    /// Palabra o frase en español. Cualquiera sirve (el detector es de
    /// vocabulario abierto), pero mejor 2-4 sílabas distintivas y no una
    /// palabra de uso común.
    pub palabra: String,
    /// Otras formas en que el reconocedor puede oír la palabra (por ejemplo
    /// «hoy asistente» para «oye asistente»); cualquiera de ellas activa.
    pub alias: Vec<String>,
    /// Umbral de detección del modelo (0-1). Más alto, menos falsos
    /// positivos y más activaciones perdidas.
    pub umbral: f32,
    /// Refuerzo (`boosting score`) de la palabra en la búsqueda. Más alto,
    /// más fácil de detectar.
    pub puntuacion: f32,
    /// Segundos tras una activación durante los que se ignoran otras.
    pub enfriamiento_s: f32,
    /// Exige que el VAD haya visto voz justo antes de aceptar la detección.
    pub exigir_voz: bool,
    /// Transductor streaming en español usado como detector.
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

impl Default for PalabraActivacionConfig {
    fn default() -> Self {
        let dir = voz_dir().join("kws-zipformer-es-kroko");
        Self {
            activada: false,
            palabra: "hola asistente".to_string(),
            alias: Vec::new(),
            umbral: 0.25,
            puntuacion: 1.0,
            enfriamiento_s: 3.0,
            exigir_voz: true,
            encoder: dir.join("encoder.onnx"),
            decoder: dir.join("decoder.onnx"),
            joiner: dir.join("joiner.onnx"),
            tokens: dir.join("tokens.txt"),
        }
    }
}

impl PalabraActivacionConfig {
    pub fn verificar(&self) -> anyhow::Result<()> {
        for (nombre, ruta) in [
            ("encoder", &self.encoder),
            ("decoder", &self.decoder),
            ("joiner", &self.joiner),
            ("tokens", &self.tokens),
        ] {
            anyhow::ensure!(
                ruta.is_file(),
                "falta el fichero '{nombre}' del modelo de palabra de activación en {} \
                 (ver README, sección Voz)",
                ruta.display()
            );
        }
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.umbral) && self.enfriamiento_s >= 0.0,
            "umbral debe estar entre 0 y 1 y enfriamiento_s no puede ser negativo"
        );
        Ok(())
    }
}

pub fn voz_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
        .map(|dirs| dirs.data_dir().join("voz"))
        .unwrap_or_else(|| PathBuf::from("voz"))
}

impl Default for VozConfig {
    fn default() -> Self {
        let dir = voz_dir();
        Self {
            whisper_encoder: dir.join("whisper-small/small-encoder.int8.onnx"),
            whisper_decoder: dir.join("whisper-small/small-decoder.int8.onnx"),
            whisper_tokens: dir.join("whisper-small/small-tokens.txt"),
            vits_model: dir.join("vits-coqui-es-css10/model.onnx"),
            vits_tokens: dir.join("vits-coqui-es-css10/tokens.txt"),
            vits_data_dir: None,
            vits_lexicon: None,
            vad_model: dir.join("silero_vad.onnx"),
            palabra_activacion: PalabraActivacionConfig::default(),
        }
    }
}

impl VozConfig {
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
pub fn load() -> VozConfig {
    let Some(path) = config_path() else {
        return VozConfig::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => {
                tracing::info!(path = %path.display(), "configuración de voz cargada");
                config
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "voz.toml inválido, usando defaults");
                VozConfig::default()
            }
        },
        Err(_) => VozConfig::default(),
    }
}
