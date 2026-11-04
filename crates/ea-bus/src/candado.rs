//! Semáforo del worktree: `flock` exclusivo sobre un `.lock` por worktree.
//! Dentro del lock se escribe quién lo tiene, para poder monitorizarlo.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::bus::raiz_estado;
use crate::error::{ErrorBus, Resultado};
use crate::fs_atomico::escribir_atomico;
use crate::tipos::Rol;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ocupante {
    /// Id del agente (`qwen-code`, `codex`…); en semáforos de antes, el rol
    /// (`modelo-local`).
    pub agente: String,
    pub trabajo: String,
    pub pid: u32,
    pub desde: String,
    /// El rol con el que lo ocupa (spec 017, RF-008).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rol: Option<Rol>,
}

impl Ocupante {
    /// Lo ocupa el ejecutor (también en un semáforo de antes).
    pub fn es_ejecutor(&self) -> bool {
        self.rol == Some(Rol::Ejecutor) || Rol::desde(&self.agente) == Some(Rol::Ejecutor)
    }
}

#[derive(Debug)]
pub struct CandadoWorktree {
    fichero: File,
    ruta_lock: PathBuf,
}

fn dir_semaforos(raiz: &Path) -> PathBuf {
    raiz.join("semaforos")
}

/// Ruta del `.lock` de un worktree: hash corto de su ruta canónica.
pub fn ruta_lock(raiz: &Path, worktree: &Path) -> PathBuf {
    let canonica = ea_plataforma::rutas::canonica(worktree).unwrap_or_else(|_| worktree.to_path_buf());
    let h = ea_plataforma::fs::fnv64(canonica.to_string_lossy().as_bytes()) & 0xffff_ffff;
    dir_semaforos(raiz).join(format!("worktree-{h:08x}.lock"))
}

impl CandadoWorktree {
    pub fn adquirir_en(raiz: &Path, ruta_worktree: &Path, mut ocupante: Ocupante, espera: Duration) -> Resultado<Self> {
        let ruta = ruta_lock(raiz, ruta_worktree);
        fs::create_dir_all(ruta.parent().unwrap())?;
        let f = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&ruta)?;
        let limite = Instant::now() + espera;
        loop {
            match f.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) => {
                    if Instant::now() >= limite {
                        return Err(ErrorBus::Ocupado);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
        ocupante.pid = std::process::id();
        // Quién lo tiene va en un fichero aparte: en Windows el bloqueo es
        // obligatorio y nadie más podría leer el `.lock` mientras está tomado.
        let _ = f.set_len(0);
        let lado = ruta_ocupante(&ruta);
        // Ya tenemos el lock: el ocupante que hubiera es el residuo de un
        // proceso muerto y un lector no debe verlo en la ventana de abajo.
        let _ = fs::remove_file(&lado);
        escribir_atomico(&lado, &serde_json::to_vec(&ocupante)?)?;
        Ok(Self { fichero: f, ruta_lock: ruta })
    }

    pub fn ruta(&self) -> &Path {
        &self.ruta_lock
    }

    pub fn ocupante_en(raiz: &Path, ruta_worktree: &Path) -> Option<Ocupante> {
        leer_ocupante(&ruta_lock(raiz, ruta_worktree))
    }
}

/// Dónde se anota quién tiene un `.lock`.
fn ruta_ocupante(ruta_lock: &Path) -> PathBuf {
    ruta_lock.with_extension("ocupante")
}

/// `None` si el lock no existe o nadie lo tiene (el contenido sería residuo).
fn leer_ocupante(ruta: &Path) -> Option<Ocupante> {
    let f = File::open(ruta).ok()?;
    match f.try_lock_shared() {
        Ok(()) => {
            let _ = f.unlock();
            None
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            let texto = fs::read_to_string(ruta_ocupante(ruta)).ok()?;
            let o: Ocupante = serde_json::from_str(&texto).ok()?;
            // Un ocupante cuyo proceso ya no vive es residuo, no un dueño.
            ea_plataforma::proceso::vive(o.pid).then_some(o)
        }
        Err(_) => None,
    }
}

/// Semáforos ocupados ahora mismo en la raíz dada: (fichero, ocupante).
pub fn listar_semaforos() -> Vec<(PathBuf, Ocupante)> {
    listar_semaforos_en(&raiz_estado())
}

/// Igual que [`listar_semaforos`] sobre una raíz explícita.
pub fn listar_semaforos_en(raiz: &Path) -> Vec<(PathBuf, Ocupante)> {
    let mut v: Vec<_> = fs::read_dir(dir_semaforos(raiz))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "lock"))
        .filter_map(|p| leer_ocupante(&p).map(|o| (p, o)))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

impl Drop for CandadoWorktree {
    fn drop(&mut self) {
        let _ = fs::remove_file(ruta_ocupante(&self.ruta_lock));
        let _ = self.fichero.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ocupante(pid: u32) -> Ocupante {
        Ocupante { agente: "x".into(), trabajo: "t".into(), pid, desde: "ahora".into(), rol: None }
    }

    #[test]
    fn un_ocupante_de_un_proceso_muerto_se_descarta_y_se_borra_al_adquirir() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        let ruta = ruta_lock(d.path(), &wt);
        fs::create_dir_all(ruta.parent().unwrap()).unwrap();
        // Residuo de un proceso muerto (pid imposible) con el lock tomado por otro.
        let f = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&ruta).unwrap();
        f.lock().unwrap();
        fs::write(ruta_ocupante(&ruta), serde_json::to_vec(&ocupante(u32::MAX - 1)).unwrap()).unwrap();
        assert!(leer_ocupante(&ruta).is_none(), "un pid muerto no es un dueño");
        // Con uno vivo (el nuestro) sí se ve.
        fs::write(ruta_ocupante(&ruta), serde_json::to_vec(&ocupante(std::process::id())).unwrap()).unwrap();
        assert_eq!(leer_ocupante(&ruta).unwrap().pid, std::process::id());
        drop(f);

        // Al adquirir se borra el residuo antes de escribir el nuevo.
        fs::write(ruta_ocupante(&ruta), serde_json::to_vec(&ocupante(u32::MAX - 1)).unwrap()).unwrap();
        let c = CandadoWorktree::adquirir_en(d.path(), &wt, ocupante(0), Duration::from_secs(1)).unwrap();
        assert_eq!(CandadoWorktree::ocupante_en(d.path(), &wt).unwrap().pid, std::process::id());
        drop(c);
        assert!(!ruta_ocupante(&ruta).exists());
    }
}
