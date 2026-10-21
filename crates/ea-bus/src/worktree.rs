//! Worktree propio de un trabajo (spec 019, RF-007): `git worktree add
//! <estado>/worktrees/<id> -b ea/<id>` desde el repo de la orden, para que
//! varios trabajos del mismo repo corran a la vez sin pisarse.
//!
//! El ejecutor NO hace `git commit` en el worktree propio: su metadato vive
//! fuera, en `<repo>/.git/worktrees/<id>`, y `bwrap` no lo abre (art. V). El
//! sistema captura el diff contra `commit_inicio`, como con cualquier otro
//! worktree; la rama `ea/<id>` queda para que el usuario la fusione a mano.
//!
//! Git se ejecuta siempre con argv (nunca con shell) y con `--` antes de las
//! rutas.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bus::Bus;
use crate::error::{ErrorBus, Resultado};
use crate::tipos::Orden;

/// Resultado de una orden `git`: su salida estándar si salió bien, y el error
/// de git (ya recortado) si no.
fn git(args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let sal = Command::new("git")
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("no se pudo lanzar git: {e}"))?;
    if sal.status.success() {
        Ok(String::from_utf8_lossy(&sal.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&sal.stderr);
        let err = err.trim();
        Err(if err.is_empty() { format!("git terminó con {}", sal.status) } else { err.chars().take(400).collect() })
    }
}

fn git_en(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut v: Vec<&std::ffi::OsStr> = vec!["-C".as_ref(), dir.as_os_str()];
    v.extend(args.iter().map(|a| std::ffi::OsStr::new(*a)));
    git(&v)
}

fn comun(dir: &Path) -> Result<PathBuf, String> {
    let c = git_en(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Ok(std::fs::canonicalize(&c).unwrap_or_else(|_| PathBuf::from(c)))
}

impl Bus {
    fn ruta_worktree(&self, id: &str) -> Resultado<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(ErrorBus::Argumentos(format!("id de trabajo inválido: {id:?}")));
        }
        Ok(self.raiz.join("worktrees").join(id))
    }

    /// El directorio donde trabaja la orden. Sin `worktree_propio` es el de la
    /// orden, tal cual. Con él, crea (o reutiliza) `<estado>/worktrees/<id>`
    /// en la rama `ea/<id>`, partiendo del `HEAD` del repo de la orden.
    ///
    /// Es idempotente: si el directorio ya es un worktree de ese repo lo
    /// reutiliza; si la rama existe pero el directorio no, la vuelve a sacar
    /// sin recrearla ni pisarla. Si la ruta de la orden no es un repo git con
    /// algún commit, devuelve un error con el motivo (el ejecutor bloquea).
    pub fn preparar_worktree(&self, id: &str, orden: &Orden) -> Resultado<PathBuf> {
        if !orden.worktree_propio {
            return Ok(PathBuf::from(&orden.worktree));
        }
        let ruta = self.ruta_worktree(id)?;
        let repo = Path::new(&orden.worktree);
        let falla = |m: String| ErrorBus::Argumentos(format!("worktree propio de {id}: {m}"));
        if git_en(repo, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]).is_err() {
            return Err(falla(format!("{} no es un repositorio git con algún commit", repo.display())));
        }
        if ruta.exists() {
            let del_repo = git_en(&ruta, &["rev-parse", "--path-format=absolute", "--show-toplevel"])
                .ok()
                .and_then(|t| std::fs::canonicalize(t).ok())
                .zip(std::fs::canonicalize(&ruta).ok())
                .is_some_and(|(a, b)| a == b)
                && comun(&ruta).ok().zip(comun(repo).ok()).is_some_and(|(a, b)| a == b);
            return if del_repo {
                Ok(ruta)
            } else {
                Err(falla(format!("{} existe pero no es un worktree de {}", ruta.display(), repo.display())))
            };
        }
        std::fs::create_dir_all(ruta.parent().unwrap_or(&self.raiz))?;
        // Registro de un worktree cuyo directorio se borró a mano: fuera, o
        // git se niega a volver a sacar la rama.
        let _ = git_en(repo, &["worktree", "prune"]);
        let rama = format!("ea/{id}");
        let existe = git_en(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{rama}")]).is_ok();
        let r = if existe {
            git(&["-C".as_ref(), repo.as_os_str(), "worktree".as_ref(), "add".as_ref(), "--".as_ref(), ruta.as_os_str(), rama.as_ref()])
        } else {
            git(&[
                "-C".as_ref(),
                repo.as_os_str(),
                "worktree".as_ref(),
                "add".as_ref(),
                "-b".as_ref(),
                rama.as_ref(),
                "--".as_ref(),
                ruta.as_os_str(),
                "HEAD".as_ref(),
            ])
        };
        r.map_err(falla)?;
        Ok(ruta)
    }

    /// Quita el worktree propio de un trabajo con `git worktree remove` (sin
    /// `--force`: si tiene cambios sin guardar o ficheros nuevos, git se
    /// niega y el error sube). No borra la rama `ea/<id>`. Devuelve `false` si
    /// no había worktree. Aún no la llama nadie (la limpieza es posterior).
    pub fn limpiar_worktree(&self, id: &str) -> Resultado<bool> {
        let ruta = self.ruta_worktree(id)?;
        if !ruta.exists() {
            return Ok(false);
        }
        let falla = |m: String| ErrorBus::Argumentos(format!("limpiar el worktree de {id}: {m}"));
        let comun = comun(&ruta).map_err(falla)?;
        git(&["--git-dir".as_ref(), comun.as_os_str(), "worktree".as_ref(), "remove".as_ref(), "--".as_ref(), ruta.as_os_str()])
            .map_err(falla)?;
        Ok(true)
    }
}
