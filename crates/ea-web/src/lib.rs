//! Interfaz web del ecosistema de asistente: una API JSON local y una
//! página única que la consume. Respeta el invariante de §12.2 de la
//! especificación — para leer consulta el diario y las tareas, para actuar
//! crea tareas o decide propuestas — y nunca habla con Claude Code ni con el
//! modelo local. Las únicas rutas que salen de la base de datos son la voz
//! (que reenvía al orquestador, dueño del audio) y el reinicio de servicios.

use std::path::PathBuf;
use std::time::Duration;

use ea_store::Store;
use tokio::sync::Mutex;

pub mod api;
pub mod estado;
pub mod tema;

const DEFAULT_PORT: u16 = 8092;
const ENV_PORT: &str = "EA_WEB_PORT";

pub struct WebConfig {
    pub puerto: u16,
    /// Puerto de la superficie de voz de `ea-orchestrator`.
    pub voz_puerto: u16,
    /// Antigüedad máxima del último heartbeat para considerar vivo al
    /// orquestador (el doble del intervalo, igual que `ea status`).
    pub umbral_heartbeat_s: i64,
    /// `colors.toml` del tema activo de Omarchy.
    pub tema_path: PathBuf,
    pub rutinas_dir: PathBuf,
    /// La bóveda que se muestra y en la que se promueve.
    pub boveda: ea_boveda::Boveda,
    /// Si es falso no se lanza ningún `systemctl` (tests, o una máquina sin
    /// `systemd --user`): los servicios se reportan como desconocidos.
    pub systemctl: bool,
}

impl WebConfig {
    pub fn load() -> Self {
        let orquestador = ea_orchestrator::config::load();
        let tema_path = directories::BaseDirs::new()
            .and_then(|d| d.state_dir().map(|s| s.to_path_buf()))
            .unwrap_or_default()
            .join("omarchy/current/theme/colors.toml");
        Self {
            puerto: std::env::var(ENV_PORT)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_PORT),
            voz_puerto: orquestador.voz_puerto,
            umbral_heartbeat_s: (orquestador.intervalo_heartbeat_s * 2) as i64,
            tema_path,
            rutinas_dir: orquestador.rutinas_dir,
            boveda: ea_boveda::Boveda::configurada(),
            systemctl: true,
        }
    }
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub boveda: ea_boveda::Boveda,
    pub config: WebConfig,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(store: Store, config: WebConfig) -> Self {
        Self {
            store: Mutex::new(store),
            boveda: config.boveda.clone(),
            config,
            http: reqwest::Client::builder()
                // El ciclo de voz completo (grabar, transcribir, preguntar,
                // sintetizar) tarda mucho más que una petición normal.
                .timeout(Duration::from_secs(60))
                .build()
                .expect("cliente HTTP"),
        }
    }
}
