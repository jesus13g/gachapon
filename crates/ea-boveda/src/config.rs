//! Dónde está la raíz de la bóveda del ecosistema: `rutas.boveda` de los
//! ajustes (`ea-ajustes`), que resuelve, por este orden, la variable de
//! entorno (`EA_BOVEDA_DIR`, para pruebas y usos puntuales), `ajustes.toml`,
//! el heredado `~/.config/ecosistema-asistente/boveda.toml` (`raiz = "..."`)
//! y por último un directorio propio del ecosistema. El valor por defecto NO
//! es la bóveda personal de Obsidian del usuario: apuntarla allí es una
//! decisión suya, explícita (ver README).

use std::path::PathBuf;

pub const ENV_RAIZ: &str = "EA_BOVEDA_DIR";

/// El `boveda.toml` heredado (se sigue leyendo; ya no hace falta escribirlo).
pub fn ruta_config() -> Option<PathBuf> {
    Some(ea_ajustes::Fuentes::sistema().dir_config.join("boveda.toml"))
}

/// Raíz por defecto: `~/.local/share/ecosistema-asistente/boveda/`.
pub fn raiz_por_defecto() -> PathBuf {
    ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::sistema()).rutas.boveda
}

/// Resuelve la raíz. Un fichero ilegible o un valor inválido caen a la capa
/// siguiente con un aviso (`ea ajustes ver`): un servicio no debe dejar de
/// arrancar por eso.
pub fn raiz() -> PathBuf {
    ea_ajustes::cargar().rutas.boveda
}
