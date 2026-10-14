//! Configuración de `ea-orchestrator`: cadencia del heartbeat, en fichero
//! TOML opcional con defaults sensatos si no existe.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Directorio de ficheros de rutina (los gestionan `ea-acciones` y la CLI).
    pub rutinas_dir: PathBuf,
    pub intervalo_sondeo_s: u64,
    pub intervalo_heartbeat_s: u64,
}

impl Default for Config {
    fn default() -> Self {
        let rutinas_dir = directories::ProjectDirs::from("", "", "ecosistema-asistente")
            .map(|dirs| dirs.config_dir().join("rutinas"))
            .unwrap_or_else(|| PathBuf::from("rutinas"));
        Self { rutinas_dir, intervalo_sondeo_s: 5, intervalo_heartbeat_s: 30 }
    }
}
fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
        .map(|dirs| dirs.config_dir().join("orchestrator.toml"))
}

/// Carga `orchestrator.toml` si existe y es válido; si no existe o falla el
/// parseo, cae en los defaults documentados (no es un error fatal: el
/// orquestador debe seguir arrancando).
pub fn load() -> Config {
    let Some(path) = config_path() else {
        tracing::warn!("no se pudo resolver el directorio de configuración; usando defaults");
        return Config::default();
    };

    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(config) => {
                tracing::info!(path = %path.display(), "configuración cargada");
                config
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), %err, "config inválida, usando defaults");
                Config::default()
            }
        },
        Err(_) => {
            tracing::info!(path = %path.display(), "sin fichero de configuración, usando defaults");
            Config::default()
        }
    }
}
