//! Configuración de `ea-orchestrator`: cadencia del sondeo y del heartbeat
//! y directorio de rutinas. Sale de `[orquestador]` de los ajustes
//! (`ea-ajustes`), que también leen el `orchestrator.toml` heredado.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    /// Directorio de ficheros de rutina (los gestionan `ea-acciones` y la CLI).
    pub rutinas_dir: PathBuf,
    pub intervalo_sondeo_s: u64,
    pub intervalo_heartbeat_s: u64,
}

impl From<ea_ajustes::Orquestador> for Config {
    fn from(o: ea_ajustes::Orquestador) -> Self {
        Self { rutinas_dir: o.rutinas_dir, intervalo_sondeo_s: o.intervalo_sondeo_s, intervalo_heartbeat_s: o.intervalo_heartbeat_s }
    }
}

impl Default for Config {
    fn default() -> Self {
        ea_ajustes::Ajustes::por_defecto(&ea_ajustes::Fuentes::sistema()).orquestador.into()
    }
}

/// Los ajustes efectivos de ahora. Nunca falla: lo inválido cae a la capa
/// siguiente (el orquestador debe seguir arrancando).
pub fn load() -> Config {
    ea_ajustes::cargar().orquestador.into()
}
