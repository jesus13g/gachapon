//! Dónde está la raíz de la bóveda del ecosistema. Mismo patrón que los demás
//! componentes: variable de entorno (`EA_BOVEDA_DIR`, para pruebas y usos
//! puntuales), después `~/.config/ecosistema-asistente/boveda.toml`
//! (`raiz = "..."`), y por último un directorio propio del ecosistema. El
//! valor por defecto NO es la bóveda personal de Obsidian del usuario:
//! apuntarla allí es una decisión suya, explícita (ver README).

use std::path::PathBuf;

use serde::Deserialize;

pub const ENV_RAIZ: &str = "EA_BOVEDA_DIR";

#[derive(Debug, Default, Deserialize)]
struct Fichero {
    raiz: Option<String>,
}

fn dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "ecosistema-asistente")
}

pub fn ruta_config() -> Option<PathBuf> {
    dirs().map(|d| d.config_dir().join("boveda.toml"))
}

/// Raíz por defecto: `~/.local/share/ecosistema-asistente/boveda/`.
pub fn raiz_por_defecto() -> PathBuf {
    dirs().map(|d| d.data_dir().join("boveda")).unwrap_or_else(|| PathBuf::from("boveda"))
}

/// Expande un `~/` inicial (el TOML lo escribe una persona).
fn expandir(texto: &str) -> PathBuf {
    if let Some(resto) = texto.strip_prefix("~/") {
        if let Some(base) = directories::BaseDirs::new() {
            return base.home_dir().join(resto);
        }
    }
    PathBuf::from(texto)
}

/// Resuelve la raíz: entorno > `boveda.toml` > por defecto. Un fichero
/// ilegible o inválido cae al valor por defecto (como `orchestrator.toml`):
/// un servicio no debe dejar de arrancar por eso.
pub fn raiz() -> PathBuf {
    if let Ok(v) = std::env::var(ENV_RAIZ) {
        if !v.trim().is_empty() {
            return expandir(v.trim());
        }
    }
    if let Some(ruta) = ruta_config() {
        if let Ok(texto) = std::fs::read_to_string(&ruta) {
            match toml::from_str::<Fichero>(&texto) {
                Ok(Fichero { raiz: Some(r) }) if !r.trim().is_empty() => return expandir(r.trim()),
                Ok(_) => {}
                Err(e) => eprintln!("boveda.toml inválido ({}): {e}; usando la raíz por defecto", ruta.display()),
            }
        }
    }
    raiz_por_defecto()
}
