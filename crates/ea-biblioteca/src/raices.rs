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
    /// `$XDG_RUNTIME_DIR` (ficheros efímeros de sesión de la F5).
    pub runtime: Option<PathBuf>,
    /// Repo del ecosistema (`claude/skills`, `qwen/skills`), si se conoce.
    pub repo: Option<PathBuf>,
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
        Self { home, proyecto, estado, runtime, repo }
    }

    /// Las del entorno real: `$HOME`, el directorio actual, `$EA_ESTADO_DIR`
    /// (o `~/.local/state/ecosistema-asistente`), `$XDG_RUNTIME_DIR` y
    /// `$EA_REPO_DIR` para el repo si no es el directorio actual.
    pub fn desde_entorno() -> Self {
        let var = |n: &str| std::env::var_os(n).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = var("HOME").unwrap_or_default();
        let estado = var("EA_ESTADO_DIR").unwrap_or_else(|| {
            var("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")).join("ecosistema-asistente")
        });
        let mut r = Self::nuevo(home, std::env::current_dir().ok(), estado, var("XDG_RUNTIME_DIR"));
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
            let destino = std::fs::canonicalize(&p).ok()?;
            let skills = destino.parent()?;
            let claude = skills.parent()?;
            let repo = claude.parent()?;
            (skills.file_name()? == "skills" && claude.file_name()? == "claude" && es_repo(repo)).then(|| repo.to_path_buf())
        })
    }
}
