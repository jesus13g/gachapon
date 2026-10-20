//! Interfaz web del ecosistema de asistente: una API JSON local y una
//! página única que la consume. Respeta el invariante de §12.2 de la
//! especificación — para leer consulta el diario y el bus de trabajos, para
//! actuar decide propuestas y gestiona trabajos — y nunca habla con Claude Code ni con el
//! modelo local (solo mira qué sesiones de Claude Code hay abiertas). Las únicas
//! rutas que salen de la base de datos son el reinicio de servicios, la
//! memoria de Claude Code, el editor de la bóveda (este, siempre a través
//! de `ea-boveda`, su único escritor) y los ajustes (`ajustes.toml`, a través
//! de `ea-ajustes`), que el usuario edita a mano desde la página.

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
    /// De dónde se leen (en cada petición) y adónde se escriben los ajustes.
    pub ajustes: ea_ajustes::Fuentes,
}

impl WebConfig {
    pub fn load() -> Self {
        let fuentes = ea_ajustes::Fuentes::sistema();
        let efectivos = ea_ajustes::cargar_de(&fuentes);
        for aviso in &efectivos.avisos {
            tracing::warn!(%aviso, "ajustes");
        }
        let a = efectivos.ajustes;
        let tema_path = directories::BaseDirs::new()
            .and_then(|d| d.state_dir().map(|s| s.to_path_buf()))
            .unwrap_or_default()
            .join("omarchy/current/theme/colors.toml");
        Self {
            // `red.puerto_web` (`EA_WEB_PORT` le gana); el host, siempre 127.0.0.1.
            puerto: a.red.puerto_web,
            umbral_heartbeat_s: a.umbral_latido_s(),
            tema_path,
            rutinas_dir: a.orquestador.rutinas_dir.clone(),
            boveda: ea_boveda::Boveda::en(a.rutas.boveda.clone()),
            systemctl: true,
            sesiones_dir: directories::BaseDirs::new().map(|d| d.home_dir().join(".claude/sessions")),
            // Junto al estado del bus, como el hook (que lee EA_ESTADO_DIR).
            subagentes_dir: Some(a.rutas.estado.join("subagentes")),
            proyectos_claude_dir: directories::BaseDirs::new().map(|d| d.home_dir().join(".claude/projects")),
            estado_dir: a.rutas.estado,
            ajustes: fuentes,
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
    /// Serializa las escrituras de `ajustes.toml` desde la página.
    pub ajustes_escritura: Mutex<()>,
}

impl AppState {
    pub fn new(store: Store, config: WebConfig) -> ea_store::StoreResult<Self> {
        Ok(Self {
            acciones: Mutex::new(store.duplicar()?),
            store: Mutex::new(store),
            boveda: config.boveda.clone(),
            config,
            ajustes_escritura: Mutex::new(()),
        })
    }
}
