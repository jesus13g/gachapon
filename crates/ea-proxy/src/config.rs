//! Configuración de `ea-proxy`: perfiles de generación, endpoints y puerto
//! propio. Declarada en fichero TOML (§5.2 de la especificación: "perfiles
//! declarados en configuración"), con defaults sensatos si el fichero no
//! existe — arranca sin pasos manuales previos, y el fichero solo hace
//! falta para sobreescribir algo.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub puerto: u16,
    pub llama_endpoint: String,
    pub mcp_endpoint: String,
    pub perfiles: HashMap<String, Profile>,
}

impl Default for Config {
    fn default() -> Self {
        let mut perfiles = HashMap::new();
        perfiles.insert(
            "rapido".to_string(),
            Profile {
                temperature: 0.2,
                max_tokens: 256,
            },
        );
        perfiles.insert(
            "largo".to_string(),
            Profile {
                temperature: 0.4,
                max_tokens: 1024,
            },
        );
        Self {
            puerto: 8090,
            llama_endpoint: "http://127.0.0.1:8080".to_string(),
            mcp_endpoint: "http://127.0.0.1:8765/mcp".to_string(),
            perfiles,
        }
    }
}

impl Config {
    pub fn profile(&self, name: &str) -> &Profile {
        self.perfiles
            .get(name)
            .or_else(|| self.perfiles.get("rapido"))
            .expect("el perfil 'rapido' siempre está presente por defecto")
    }
}

fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
        .map(|dirs| dirs.config_dir().join("proxy.toml"))
}

/// Carga la configuración desde `proxy.toml` si existe y es válido; si no
/// existe o falla el parseo, cae en los defaults documentados (no es un
/// error fatal — el proxy debe seguir arrancando).
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
