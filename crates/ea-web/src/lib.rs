//! Interfaz web del ecosistema de asistente: una API JSON local y una
//! página única que la consume. Respeta el invariante de §12.2 de la
//! especificación — para leer consulta el diario y el bus de trabajos, para
//! actuar decide propuestas y gestiona trabajos — y nunca habla con Claude Code ni con el
//! modelo local (solo mira qué sesiones de Claude Code hay abiertas). Las únicas
//! rutas que salen de la base de datos son el reinicio de servicios, la
//! memoria de Claude Code y el editor de la bóveda (este, siempre a través
//! de `ea-boveda`, su único escritor), que el usuario edita a mano desde la
//! página.

use std::path::PathBuf;

use ea_store::Store;
use tokio::sync::Mutex;

pub mod estado;
mod git;
pub mod http;
pub mod memoria;
pub mod servicio;
pub mod sesiones;
pub mod tema;
pub mod vistas;

/// Nombre histórico de la capa HTTP (`ea_web::api::router`).
pub use http as api;

const DEFAULT_PORT: u16 = 8092;
const ENV_PORT: &str = "EA_WEB_PORT";

pub struct WebConfig {
    pub puerto: u16,
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
    /// Dónde Claude Code deja un fichero por sesión abierta. `None` = no se
    /// leen sesiones (tests).
    pub sesiones_dir: Option<PathBuf>,
    /// Dónde el hook `claude/hooks/ea-subagentes.sh` apunta los subagentes de
    /// cada sesión. `None` = no se leen (tests).
    pub subagentes_dir: Option<PathBuf>,
    /// Dónde guarda Claude Code cada proyecto con su memoria
    /// (`<id>/memory/*.md`). `None` = no se lee (tests sin fixture).
    pub proyectos_claude_dir: Option<PathBuf>,
    /// Raíz de estado del bus de trabajos (colas, trabajos, semáforos).
    pub estado_dir: PathBuf,
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
            umbral_heartbeat_s: (orquestador.intervalo_heartbeat_s * 2) as i64,
            tema_path,
            rutinas_dir: orquestador.rutinas_dir,
            boveda: ea_boveda::Boveda::configurada(),
            systemctl: true,
            sesiones_dir: directories::BaseDirs::new().map(|d| d.home_dir().join(".claude/sessions")),
            subagentes_dir: directories::BaseDirs::new()
                .and_then(|d| d.state_dir().map(|s| s.join("ecosistema-asistente/subagentes"))),
            proyectos_claude_dir: directories::BaseDirs::new().map(|d| d.home_dir().join(".claude/projects")),
            estado_dir: ea_bus::raiz_estado(),
        }
    }
}

pub struct AppState {
    pub store: Mutex<Store>,
    /// Conexión aparte para las acciones supervisadas. Una acción puede
    /// esperar hasta 30 s a `systemctl` con su candado cogido; con una sola
    /// conexión, la página entera se quedaba sin refrescar mientras tanto.
    /// Las acciones siguen serializadas entre sí.
    pub acciones: Mutex<Store>,
    pub boveda: ea_boveda::Boveda,
    pub config: WebConfig,
}

impl AppState {
    pub fn new(store: Store, config: WebConfig) -> ea_store::StoreResult<Self> {
        Ok(Self {
            acciones: Mutex::new(store.duplicar()?),
            store: Mutex::new(store),
            boveda: config.boveda.clone(),
            config,
        })
    }
}
