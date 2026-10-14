//! Configuración del orquestador: `~/.config/ecosistema-asistente/orquestador.toml`.
//! Es el ÚNICO sitio del código donde se nombra el modelo.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::tipos::Topes;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ConfigOrquestador {
    pub base_url: String,
    pub modelo: String,
    pub qwen_bin: String,
    pub topes: Topes,
    pub avisos_escritorio: bool,
    /// Ejecuta qwen dentro de bubblewrap (solo el worktree es escribible).
    pub sandbox: bool,
}

impl Default for ConfigOrquestador {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8080/v1".into(),
            modelo: "qwen3-coder-30b".into(),
            qwen_bin: "qwen".into(),
            topes: Topes::default(),
            avisos_escritorio: true,
            sandbox: true,
        }
    }
}

pub fn ruta_config() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "ecosistema-asistente").map(|d| d.config_dir().join("orquestador.toml"))
}

impl ConfigOrquestador {
    /// Carga la configuración estándar; si falta o es inválida, los valores por defecto.
    pub fn cargar() -> Self {
        ruta_config().map(|r| Self::cargar_de(&r)).unwrap_or_default()
    }

    pub fn cargar_de(ruta: &Path) -> Self {
        match std::fs::read_to_string(ruta) {
            Ok(texto) => toml::from_str(&texto).unwrap_or_else(|e| {
                eprintln!("orquestador.toml inválido ({}): {e}; usando los valores por defecto", ruta.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
}
