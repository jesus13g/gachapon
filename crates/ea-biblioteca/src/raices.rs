//! Dónde mira la biblioteca. Toda ruta sale de aquí para que las pruebas usen
//! un HOME de `tempfile` y nunca el real.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Raices {
    pub home: PathBuf,
    /// El proyecto (repo o carpeta) en que se ejecuta `ea`; `None`: sin ámbito de proyecto.
    pub proyecto: Option<PathBuf>,
    /// Estado del ecosistema (copias y almacén de desactivadas de la F4).
    pub estado: PathBuf,
    /// Directorio de ejecución del usuario (Linux: `$XDG_RUNTIME_DIR`; ficheros efímeros de sesión de la F5).
    pub runtime: Option<PathBuf>,
    /// Repo del ecosistema (`claude/skills`, `qwen/skills`), si se conoce.
    pub repo: Option<PathBuf>,
    /// Base de datos del ecosistema (el diario, para el uso por actor).
    pub db: PathBuf,
}

/// ¿`dir` es el repo del ecosistema?
fn es_repo(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file() && dir.join("claude/skills").is_dir() && dir.join("qwen/skills").is_dir()
}

impl Raices {
    /// Constructor explícito (pruebas y quien ya sabe las rutas). El repo se
    /// deduce del proyecto si lo es; se puede fijar a mano en `repo`.
    pub fn nuevo(home: PathBuf, proyecto: Option<PathBuf>, estado: PathBuf, runtime: Option<PathBuf>) -> Self {
        let proyecto = proyecto.filter(|p| *p != home);
        let repo = proyecto.clone().filter(|p| es_repo(p));
        let db = home.join(".local/share/ecosistema-asistente/ecosistema.db");
        Self { home, proyecto, estado, runtime, repo, db }
    }

    /// Las del entorno real: el home del usuario (`ea_plataforma::usuario::home`),
    /// el directorio actual, el estado del ecosistema y su base de datos
    /// (`rutas.estado` y `rutas.db` de los ajustes, que resuelven
    /// `$EA_ESTADO_DIR` y `$EA_DB_PATH`: los mismos que usan el bus y el
    /// diario), el directorio de ejecución de la plataforma y `$EA_REPO_DIR`
    /// para el repo si no es el directorio actual.
    pub fn desde_entorno() -> Self {
        let var = |n: &str| std::env::var_os(n).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = ea_plataforma::usuario::home().or_else(|| var("HOME")).unwrap_or_default();
        let rutas = ea_ajustes::cargar().rutas;
        let runtime = ea_ajustes::dir_ejecucion(&ea_ajustes::Entorno::Proceso);
        let mut r = Self::nuevo(home, std::env::current_dir().ok(), rutas.estado, runtime);
        r.db = rutas.db;
        if r.repo.is_none() {
            r.repo = var("EA_REPO_DIR").filter(|p| es_repo(p));
        }
        r
    }

    /// El repo del ecosistema: el fijado o, si no, el que señalan los enlaces
    /// simbólicos de `~/.claude/skills` hacia `<repo>/claude/skills/<n>`.
    pub fn repo_efectivo(&self) -> Option<PathBuf> {
        if let Some(r) = &self.repo {
            return Some(r.clone());
        }
        let mut entradas: Vec<PathBuf> = std::fs::read_dir(self.home.join(".claude/skills")).ok()?.flatten().map(|e| e.path()).collect();
        entradas.sort();
        entradas.into_iter().find_map(|p| {
            let destino = ea_plataforma::rutas::canonica(&p).ok()?;
            let skills = destino.parent()?;
            let claude = skills.parent()?;
            let repo = claude.parent()?;
            (skills.file_name()? == "skills" && claude.file_name()? == "claude" && es_repo(repo)).then(|| repo.to_path_buf())
        })
    }
}
