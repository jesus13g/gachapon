//! Dónde está la raíz de la bóveda del ecosistema: `rutas.boveda` de los
//! ajustes (`ea-ajustes`), que resuelve, por este orden, la variable de
//! entorno (`EA_BOVEDA_DIR`, para pruebas y usos puntuales), `ajustes.toml`,
//! el heredado `~/.config/ecosistema-asistente/boveda.toml` (`raiz = "..."`)
//! y por último un directorio propio del ecosistema. El valor por defecto NO
//! es la bóveda personal de Obsidian del usuario: apuntarla allí es una
//! decisión suya, explícita (ver README).

use std::path::PathBuf;

/// Resuelve la raíz. Un fichero ilegible o un valor inválido caen a la capa
/// siguiente con un aviso (`ea ajustes ver`): un servicio no debe dejar de
/// arrancar por eso.
pub fn raiz() -> PathBuf {
    ea_ajustes::cargar().rutas.boveda
}
