//! Worktree propio de un trabajo (spec 019, RF-007): `git worktree add
//! <estado>/worktrees/<id> -b ea/<id>` desde el repo de la orden, para que
//! varios trabajos del mismo repo corran a la vez sin pisarse.
//!
//! El ejecutor NO hace `git commit` en el worktree propio: su metadato vive
//! fuera, en `<repo>/.git/worktrees/<id>`, y `bwrap` no lo abre (art. V). El
//! sistema captura el diff contra `commit_inicio`, como con cualquier otro
//! worktree; la rama `ea/<id>` queda para que el usuario la fusione a mano.
//!
//! Git va por [`crate::git`]: endurecido, con tope y siempre con argv (nunca
//! con shell) y `--` antes de las rutas.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::bus::Bus;
use crate::error::{ErrorBus, Resultado};
use crate::git::{self, TOPE_ESCRITURA, TOPE_LECTURA};
use crate::tipos::{EstadoTrabajo, Orden};
use serde::Serialize;

/// Una orden `git` sobre `dir`: su salida sin espacios a los lados si salió
/// bien, y lo que dijo git (recortado) si no.
fn git_en<S: AsRef<OsStr>>(dir: &Path, args: &[S], tope: Duration) -> Result<String, String> {
    git::git(dir, args, tope).map(|s| s.trim().to_string()).map_err(|e| match e {
        ErrorBus::Git { detalle, .. } => detalle.chars().take(400).collect(),
        e => e.to_string(),
    })
}

fn comun(dir: &Path) -> Result<PathBuf, String> {
    let c = git_en(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"], TOPE_LECTURA)?;
    Ok(ea_plataforma::rutas::canonica(&c).unwrap_or_else(|_| PathBuf::from(c)))
}

/// Un worktree propio vivo en `<estado>/worktrees/` (spec 019, anexo A).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InfoWorktree {
    pub trabajo: String,
    pub ruta: String,
    /// La rama `ea/<id>`.
    pub rama: String,
    /// Estado del trabajo en el bus (`None` si el directorio no es de ningún trabajo).
    pub estado_trabajo: Option<EstadoTrabajo>,
    pub titulo: Option<String>,
    /// Sin cambios ni ficheros sin seguimiento. `None` si git no pudo decirlo
    /// (directorio roto o ajeno).
    pub limpio: Option<bool>,
    /// El semáforo del ejecutor lo tiene cogido este trabajo ahora.
    pub ocupado: bool,
}

impl InfoWorktree {
    /// El trabajo terminó (`cerrado` o `cancelado`): ya no escribe en él.
    pub fn trabajo_terminado(&self) -> bool {
        matches!(self.estado_trabajo, Some(EstadoTrabajo::Cerrado | EstadoTrabajo::Cancelado))
    }
}

impl Bus {
    /// El worktree propio del trabajo, si existe su directorio. Solo lee.
    pub fn info_worktree(&self, id: &str) -> Resultado<Option<InfoWorktree>> {
        let ruta = self.ruta_worktree(id)?;
        if !ruta.is_dir() {
            return Ok(None);
        }
        let estado = self.leer_estado(id).ok().map(|e| e.estado);
        let limpio = git_en(&ruta, &["status", "--porcelain"], TOPE_LECTURA).ok().map(|s| s.is_empty());
        let ocupado = crate::candado::listar_semaforos_en(&self.raiz).iter().any(|(_, o)| o.trabajo == id);
        Ok(Some(InfoWorktree {
            trabajo: id.to_string(),
            ruta: ruta.to_string_lossy().into_owned(),
            rama: format!("ea/{id}"),
            estado_trabajo: estado,
            titulo: self.leer_orden(id).ok().map(|o| o.titulo),
            limpio,
            ocupado,
        }))
    }

    /// Los worktrees propios vivos, por id. Solo lee.
    pub fn listar_worktrees(&self) -> Resultado<Vec<InfoWorktree>> {
        let mut ids: Vec<String> = std::fs::read_dir(self.raiz.join("worktrees"))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        ids.sort();
        let mut v = Vec::new();
        for id in ids {
            if let Some(i) = self.info_worktree(&id).ok().flatten() {
                v.push(i);
            }
        }
        Ok(v)
    }

    fn ruta_worktree(&self, id: &str) -> Resultado<PathBuf> {
        if !crate::bus::id_valido(id) {
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
        if !git::tiene_commits(repo, TOPE_LECTURA).unwrap_or(false) {
            return Err(falla(format!("{} no es un repositorio git con algún commit", repo.display())));
        }
        if ruta.exists() {
            let del_repo = git_en(&ruta, &["rev-parse", "--path-format=absolute", "--show-toplevel"], TOPE_LECTURA)
                .ok()
                .and_then(|t| ea_plataforma::rutas::canonica(t).ok())
                .zip(ea_plataforma::rutas::canonica(&ruta).ok())
                .is_some_and(|(a, b)| a == b)
                && comun(&ruta).ok().zip(comun(repo).ok()).is_some_and(|(a, b)| a == b);
            return if del_repo { Ok(ruta) } else { Err(falla(format!("{} existe pero no es un worktree de {}", ruta.display(), repo.display()))) };
        }
        std::fs::create_dir_all(ruta.parent().unwrap_or(&self.raiz))?;
        // Registro de un worktree cuyo directorio se borró a mano: fuera, o
        // git se niega a volver a sacar la rama.
        let _ = git_en(repo, &["worktree", "prune"], TOPE_LECTURA);
        let rama = format!("ea/{id}");
        let existe = git_en(repo, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{rama}")], TOPE_LECTURA).is_ok();
        let r = if existe {
            git_en(repo, &["worktree".as_ref(), "add".as_ref(), "--".as_ref(), ruta.as_os_str(), rama.as_ref()], TOPE_ESCRITURA)
        } else {
            let args: [&OsStr; 7] = ["worktree".as_ref(), "add".as_ref(), "-b".as_ref(), rama.as_ref(), "--".as_ref(), ruta.as_os_str(), "HEAD".as_ref()];
            git_en(repo, &args, TOPE_ESCRITURA)
        };
        r.map_err(falla)?;
        Ok(ruta)
    }

    /// Quita el worktree propio de un trabajo con `git worktree remove` (sin
    /// `--force`: si tiene cambios sin guardar o ficheros nuevos, git se
    /// niega y el error sube). No borra la rama `ea/<id>`. Devuelve `false` si
    /// no había worktree. La llama `ea_limpiar_worktree` (spec 019, anexo A), que
    /// además exige un trabajo terminado y el worktree limpio.
    pub fn limpiar_worktree(&self, id: &str) -> Resultado<bool> {
        let ruta = self.ruta_worktree(id)?;
        if !ruta.exists() {
            return Ok(false);
        }
        let falla = |m: String| ErrorBus::Argumentos(format!("limpiar el worktree de {id}: {m}"));
        let comun = comun(&ruta).map_err(falla)?;
        let args: [&OsStr; 6] = ["--git-dir".as_ref(), comun.as_os_str(), "worktree".as_ref(), "remove".as_ref(), "--".as_ref(), ruta.as_os_str()];
        git_en(&comun, &args, TOPE_ESCRITURA).map_err(falla)?;
        Ok(true)
    }
}
