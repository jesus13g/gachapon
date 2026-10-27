//! El `git` que el sistema ejecuta fuera del sandbox sobre el worktree de un
//! trabajo: las instantáneas y el diff del ejecutor, `ea trabajo diff`, el
//! diff del panel y los worktrees propios. El agente ha podido escribir en
//! ese worktree, así que aquí no se ejecuta nada que configure el repo:
//!
//! - `-c core.fsmonitor=false` siempre: el fsmonitor es un programa del repo;
//! - `--no-ext-diff --no-textconv` en todo `diff`: ni `diff.external` ni los
//!   *drivers* `textconv` de `.gitattributes` (un binario con un `textconv`
//!   legítimo sale como «Binary files differ»);
//! - sin `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` (salvo el índice
//!   temporal de la instantánea) ni `GIT_EXTERNAL_DIFF` del entorno, y sin
//!   preguntar nada por la terminal;
//! - con tope de tiempo y muerte del árbol ([`ea_plataforma::proceso::correr`]):
//!   [`TOPE_LECTURA`] para leer y [`TOPE_ESCRITURA`] para `add -A`,
//!   `write-tree` y `worktree add`/`remove`. El panel pasa el suyo.
//!
//! No se ponen `GIT_CONFIG_NOSYSTEM` ni `core.hooksPath`: romperían git-lfs y
//! los *hooks* legítimos de `worktree add`. La configuración y los *hooks* del
//! repo se protegen en el sandbox del agente, que no puede escribir `.git`.
//!
//! Git se ejecuta siempre con argv (nunca con shell) y con `--` antes de las
//! rutas.

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::error::{ErrorBus, Resultado};
use crate::instantanea::EXCLUIR_DIFF;

/// Tope de una orden que solo lee (`rev-parse`, `diff`, `ls-files`…).
pub const TOPE_LECTURA: Duration = Duration::from_secs(30);
/// Tope de una orden que escribe objetos o un worktree entero.
pub const TOPE_ESCRITURA: Duration = Duration::from_secs(300);

/// `git -C wt <args>`, endurecido y con `tope`: su salida estándar tal cual.
/// Si git falla, [`ErrorBus::Git`] con lo que dijo; si vence el tope,
/// [`ErrorBus::GitSinRespuesta`].
pub fn git<I, S>(wt: &Path, args: I, tope: Duration) -> Resultado<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    lanzar(wt, args, None, tope)
}

/// Como [`git`], con `indice` como índice en vez del del repo.
pub fn git_con_indice<I, S>(wt: &Path, args: I, indice: &Path, tope: Duration) -> Resultado<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    lanzar(wt, args, Some(indice), tope)
}

fn lanzar<I, S>(wt: &Path, args: I, indice: Option<&Path>, tope: Duration) -> Resultado<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_os_string()).collect();
    let mut c = Command::new("git");
    c.args(["-c", "core.fsmonitor=false", "-C"]).arg(wt).args(&args);
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_EXTERNAL_DIFF"] {
        c.env_remove(v);
    }
    c.env("GIT_TERMINAL_PROMPT", "0");
    if let Some(i) = indice {
        c.env("GIT_INDEX_FILE", i);
    }
    let inicio = Instant::now();
    let r = ea_plataforma::proceso::correr(c, tope, &|| false);
    if r.codigo == Some(0) {
        return Ok(r.salida);
    }
    // Sin código antes del tope: no arrancó o lo mató una señal.
    if r.codigo.is_none() && inicio.elapsed() >= tope {
        return Err(ErrorBus::GitSinRespuesta(tope.as_secs()));
    }
    let err = r.errores.trim();
    let detalle = match r.codigo {
        _ if !err.is_empty() => err.to_string(),
        Some(n) => format!("git terminó con el código {n}"),
        None => "git no terminó".into(),
    };
    let orden = args.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
    Err(ErrorBus::Git { orden, detalle })
}

/// El commit de `HEAD` (el `commit_inicio` de un trabajo); `None` si `wt` no
/// es un repo con algún commit.
pub fn commit_actual(wt: &Path) -> Option<String> {
    git(wt, ["rev-parse", "HEAD"], TOPE_LECTURA).ok().map(|s| s.trim().to_string())
}

/// Si `wt` está dentro de un árbol de trabajo de git. Solo un tope vencido
/// es un error; cualquier otro fallo es «no».
pub fn es_repo(wt: &Path, tope: Duration) -> Resultado<bool> {
    sin_fallos(git(wt, ["rev-parse", "--is-inside-work-tree"], tope).map(|s| s.trim() == "true"))
}

/// Si el `HEAD` del repo de `wt` apunta a un commit. Solo un tope vencido es
/// un error; cualquier otro fallo es «no».
pub fn tiene_commits(wt: &Path, tope: Duration) -> Resultado<bool> {
    sin_fallos(git(wt, ["rev-parse", "--verify", "-q", "HEAD^{commit}"], tope).map(|s| !s.trim().is_empty()))
}

/// Longitud máxima de la ruta del worktree de una orden nueva.
const WORKTREE_MAX: usize = 4_096;

/// El worktree de una orden nueva: no vacío, de hasta 4096 bytes, ruta
/// absoluta, que existe y es un directorio con ruta UTF-8, dentro de un repo
/// git ([`es_repo`]) que tiene algún commit ([`tiene_commits`]; el ejecutor
/// arranca con `rev-parse HEAD` y bloquearía un repo sin commits). Devuelve
/// la forma canónica, que es la que va a la orden (y al semáforo). Lo llaman
/// `ea trabajo crear` y la web antes de crear el trabajo, con los mismos
/// mensajes ([`ErrorBus::Argumentos`]); solo un git que no responde en
/// `tope` es [`ErrorBus::GitSinRespuesta`].
pub fn validar_worktree(wt: &str, tope: Duration) -> Resultado<String> {
    let invalido = |m: String| Err(ErrorBus::Argumentos(m));
    let wt = wt.trim();
    if wt.is_empty() {
        return invalido("'worktree' no puede estar vacío".into());
    }
    if wt.len() > WORKTREE_MAX {
        return invalido(format!("'worktree' supera {WORKTREE_MAX} bytes"));
    }
    let ruta = Path::new(wt);
    if !ruta.is_absolute() {
        return invalido("'worktree' debe ser una ruta absoluta".into());
    }
    let Ok(canon) = ea_plataforma::rutas::canonica(ruta) else {
        return invalido(format!("el worktree '{wt}' no existe"));
    };
    if !canon.is_dir() {
        return invalido(format!("el worktree '{wt}' no es un directorio"));
    }
    let Some(canon) = canon.to_str().map(str::to_string) else {
        return invalido("la ruta del worktree no es UTF-8".into());
    };
    if !es_repo(Path::new(&canon), tope)? {
        return invalido(format!("'{canon}' no está dentro de un repositorio git"));
    }
    if !tiene_commits(Path::new(&canon), tope)? {
        return invalido(format!("el repo no tiene ningún commit ('{canon}'): haz un commit inicial antes de encargarle trabajo"));
    }
    Ok(canon)
}

fn sin_fallos(r: Resultado<bool>) -> Resultado<bool> {
    match r {
        Err(e @ ErrorBus::GitSinRespuesta(_)) => Err(e),
        r => Ok(r.unwrap_or(false)),
    }
}

/// El árbol de git del estado actual del worktree SIN tocar el índice del
/// usuario: `add -A` y `write-tree` con un índice temporal en `dir`.
pub fn instantanea(wt: &Path, dir: &Path) -> Option<String> {
    let idx = dir.join(".indice-tmp");
    let _ = std::fs::remove_file(&idx);
    let arbol = git_con_indice(wt, ["add", "-A"], &idx, TOPE_ESCRITURA)
        .and_then(|_| git_con_indice(wt, ["write-tree"], &idx, TOPE_ESCRITURA));
    let _ = std::fs::remove_file(&idx);
    arbol.ok().map(|s| s.trim().to_string())
}

/// `git diff [--stat] <revisiones> -- . <EXCLUIR_DIFF>`, sin drivers. Una
/// revisión que empiece por `-` no llega a git (sería una opción).
fn diff(wt: &Path, revisiones: &[&str], stat: bool, tope: Duration) -> Resultado<String> {
    if let Some(r) = revisiones.iter().find(|r| r.starts_with('-')) {
        return Err(ErrorBus::Argumentos(format!("revisión de git inválida: {r:?}")));
    }
    let mut args = vec!["diff", "--no-ext-diff", "--no-textconv"];
    if stat {
        args.push("--stat");
    }
    args.extend(revisiones);
    args.extend(["--", ".", EXCLUIR_DIFF]);
    git(wt, args, tope)
}

/// El diff (o su `--stat`) entre dos árboles o commits, tal cual sale de git:
/// el de una fase de obra entre sus instantáneas.
pub fn diff_entre(wt: &Path, a: &str, b: &str, stat: bool, tope: Duration) -> Resultado<String> {
    diff(wt, &[a, b], stat, tope)
}

/// El diff del worktree desde `inicio`, tal cual sale de git (sin los
/// ficheros nuevos sin seguimiento: ver [`ficheros_nuevos`]).
pub fn diff_desde(wt: &Path, inicio: &str, tope: Duration) -> Resultado<String> {
    diff(wt, &[inicio], false, tope)
}

/// El `diff_stat` del informe de una misión o una rutina: `git diff --stat`
/// desde `inicio` y, si los hay, «nuevos: a, b» con los ficheros sin
/// seguimiento (salvo `__pycache__`, con los nombres como los cita git).
/// Vacío si git falla.
pub fn diff_stat_desde(wt: &Path, inicio: &str) -> String {
    let leer = |args: &[&str]| git(wt, args, TOPE_LECTURA).map(|s| s.trim().to_string()).unwrap_or_default();
    let mut stat = leer(&["diff", "--no-ext-diff", "--no-textconv", "--stat", inicio]);
    let nuevos: Vec<String> = leer(&["ls-files", "--others", "--exclude-standard"])
        .lines()
        .filter(|l| !l.contains("__pycache__"))
        .map(str::to_string)
        .collect();
    if !nuevos.is_empty() {
        stat.push_str(&format!("\nnuevos: {}", nuevos.join(", ")));
    }
    stat.trim().to_string()
}

/// Un fichero nuevo sin seguimiento del worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FicheroNuevo {
    /// Ruta relativa al worktree, tal como la da git.
    pub ruta: String,
    /// Su contenido, si es texto UTF-8.
    pub texto: Option<String>,
}

impl FicheroNuevo {
    /// Cómo lo enseñan los diffs del trabajo, sin salto final: la cabecera
    /// «=== fichero nuevo sin seguimiento: … ===» y su contenido, o la
    /// cabecera con «(binario o ilegible)».
    pub fn bloque(&self) -> String {
        match &self.texto {
            Some(t) => format!("\n=== fichero nuevo sin seguimiento: {} ===\n{t}", self.ruta),
            None => format!("\n=== fichero nuevo sin seguimiento: {} (binario o ilegible) ===", self.ruta),
        }
    }
}

/// Los ficheros nuevos sin seguimiento (respetando `.gitignore`), salvo
/// `__pycache__`, en el orden de git y con su contenido si es texto.
pub fn ficheros_nuevos(wt: &Path, tope: Duration) -> Resultado<Vec<FicheroNuevo>> {
    let lista = git(wt, ["ls-files", "--others", "--exclude-standard", "-z"], tope)?;
    Ok(lista
        .split('\0')
        .filter(|f| !f.is_empty() && !f.contains("__pycache__"))
        .map(|f| FicheroNuevo {
            ruta: f.to_string(),
            texto: std::fs::read(wt.join(f)).ok().and_then(|b| String::from_utf8(b).ok()),
        })
        .collect())
}

/// Lo cambiado desde `inicio` más los ficheros nuevos sin seguimiento, como
/// lo enseña el panel: el diff y, detrás, el bloque de cada fichero nuevo
/// (el de un binario acaba en salto de línea). `ea trabajo diff` imprime lo
/// mismo, con cada bloque en su línea.
pub fn diff_trabajo(wt: &Path, inicio: &str, tope: Duration) -> Resultado<String> {
    let mut texto = diff_desde(wt, inicio, tope)?;
    for f in ficheros_nuevos(wt, tope)? {
        texto.push_str(&f.bloque());
        if f.texto.is_none() {
            texto.push('\n');
        }
    }
    Ok(texto)
}
