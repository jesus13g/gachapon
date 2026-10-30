//! Aislamiento del ejecutor por sistema (spec 020, decisión 2; art. V y VIII).
//! [`Sandbox::envolver`] es el único sitio que construye la orden aislada del
//! agente, y [`Sandbox::envolver_criterio`] la del criterio, con el mismo
//! perfil de disco (el criterio corre código que ha escrito el agente).
//!
//! - Linux: `bwrap` con los argumentos de [`argumentos_bwrap`].
//! - macOS: `sandbox-exec` con un perfil Seatbelt (SBPL) equivalente, generado
//!   por [`perfil_seatbelt`]: escritura solo en el worktree (salvo su `.git`),
//!   un temporal propio y los directorios de estado del adaptador; el
//!   directorio de ejecución, ilegible; red solo hacia `localhost`.
//! - En los dos, las rutas de `agente.tapar` (spec 024) quedan escondidas al
//!   agente y al criterio ([`resolver_tapar`]).
//! - Windows (y cualquier otro): no hay equivalente. Con `sandbox = true` el
//!   ejecutor **no arranca** y dice por qué; no se relaja en silencio.
//!
//! El `.git` del worktree es de solo lectura dentro del sandbox: el sistema
//! ejecuta git sobre ese repo fuera de él (`ea_bus::git`), y una
//! configuración o un *hook* plantados ahí correrían con los permisos del
//! usuario.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use ea_plataforma::Sistema;

/// Dónde está `sandbox-exec` en macOS.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Cómo se aísla al agente en esta corrida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sandbox {
    /// `sandbox = false`: se lanza directo (con tope de tiempo y muerte del árbol).
    Ninguno,
    Bwrap,
    Seatbelt,
    /// No hay forma de aislar: el motivo explica qué hacer.
    NoDisponible(String),
}

impl Sandbox {
    /// Elección pura: `activado` es `config.sandbox`; `hay_sandbox_exec`, si
    /// existe `/usr/bin/sandbox-exec`.
    pub fn elegir(sistema: Sistema, activado: bool, hay_sandbox_exec: bool) -> Sandbox {
        if !activado {
            return Sandbox::Ninguno;
        }
        match sistema {
            Sistema::Linux => Sandbox::Bwrap,
            Sistema::MacOs if hay_sandbox_exec => Sandbox::Seatbelt,
            Sistema::MacOs => Sandbox::NoDisponible(format!(
                "no se encuentra {SANDBOX_EXEC}, que el ejecutor necesita en macOS para aislar al agente (usa sandbox = false bajo tu responsabilidad)"
            )),
            Sistema::Windows => Sandbox::NoDisponible(
                "en Windows no hay un sandbox equivalente a bwrap; el ejecutor no arranca con sandbox = true (ver limitaciones en el README; usa WSL2 o, bajo tu responsabilidad, sandbox = false)"
                    .into(),
            ),
            Sistema::Otro => Sandbox::NoDisponible(
                "este sistema no tiene un sandbox soportado por el ejecutor; no arranca con sandbox = true (usa sandbox = false bajo tu responsabilidad)".into(),
            ),
        }
    }

    /// La elección para el sistema en el que corre el binario.
    pub fn actual(activado: bool) -> Sandbox {
        Sandbox::elegir(Sistema::actual(), activado, Path::new(SANDBOX_EXEC).is_file())
    }

    /// Lo que el latido declara al panel: sistema, aislamiento y motivo.
    pub fn info(&self, sistema: Sistema) -> ea_bus::SandboxInfo {
        let (nombre, motivo) = match self {
            Sandbox::Ninguno => ("ninguno", None),
            Sandbox::Bwrap => ("bwrap", None),
            Sandbox::Seatbelt => ("seatbelt", None),
            Sandbox::NoDisponible(m) => ("no disponible", Some(m.clone())),
        };
        ea_bus::SandboxInfo { sistema: sistema.nombre().into(), sandbox: nombre.into(), disponible: motivo.is_none(), motivo }
    }

    /// `Err(motivo)` si el ejecutor no puede arrancar con este aislamiento.
    pub fn comprobar(&self) -> Result<(), String> {
        match self {
            Sandbox::NoDisponible(m) => Err(m.clone()),
            _ => Ok(()),
        }
    }

    /// La orden que lanza `programa args` en `wt` con este aislamiento: el
    /// worktree y los directorios de `estado` escribibles, su `.git` de solo
    /// lectura y el directorio de `ejecucion` y las rutas de `tapar` tapados
    /// ([`argumentos_bwrap`], [`perfil_seatbelt`]). Sin sandbox, la orden tal
    /// cual; con `NoDisponible` o con una ruta de `tapar` que se solapa con lo
    /// escribible ([`resolver_tapar`]), `Err` con el motivo y no se construye nada.
    pub fn envolver(&self, programa: &str, args: &[String], wt: &Path, estado: &[PathBuf], ocultar: Ocultar) -> Result<Envoltorio, String> {
        self.construir(programa.as_ref(), args.iter().map(OsStr::new), wt, estado, ocultar, false)
    }

    /// La orden `cmd` del criterio de éxito con el mismo aislamiento de disco
    /// que el agente ([`Sandbox::envolver`]): `/` de solo lectura, un `/tmp`
    /// propio, el directorio de `ejecucion` y las rutas de `tapar` tapados y escribibles solo el
    /// worktree (su `.git` no) y `escribibles` (`agente.criterio.escribibles`,
    /// en su lugar de los directorios de estado del agente). La red, como
    /// hasta ahora: abierta (en macOS el perfil del criterio no lleva las
    /// reglas de red del agente). Sin sandbox, `cmd` tal cual. Conserva sus
    /// argumentos, su entorno y su directorio (`wt` si no tiene).
    pub fn envolver_criterio(&self, cmd: Command, wt: &Path, escribibles: &[PathBuf], ocultar: Ocultar) -> Result<Envoltorio, String> {
        if *self == Sandbox::Ninguno {
            return Ok(Envoltorio { orden: cmd, _temporal: None });
        }
        let mut e = self.construir(cmd.get_program(), cmd.get_args(), wt, escribibles, ocultar, true)?;
        for (k, v) in cmd.get_envs() {
            match v {
                Some(v) => e.orden.env(k, v),
                None => e.orden.env_remove(k),
            };
        }
        e.orden.current_dir(cmd.get_current_dir().unwrap_or(wt));
        Ok(e)
    }

    /// La orden aislada de `programa args`, del agente o (`criterio`) del
    /// criterio: solo cambian el perfil de red de Seatbelt y el nombre del
    /// temporal.
    fn construir<'a>(
        &self,
        programa: &OsStr,
        args: impl IntoIterator<Item = &'a OsStr>,
        wt: &Path,
        escribibles: &[PathBuf],
        ocultar: Ocultar,
        criterio: bool,
    ) -> Result<Envoltorio, String> {
        let ejecucion = ocultar.ejecucion;
        let tapar = || resolver_tapar(ocultar.tapar, wt, escribibles);
        let mut temporal = None;
        let mut orden = match self {
            Sandbox::NoDisponible(m) => return Err(m.clone()),
            Sandbox::Ninguno => Command::new(programa),
            Sandbox::Bwrap => {
                let mut c = Command::new("bwrap");
                c.args(argumentos_bwrap(wt, ejecucion, &tapar()?, escribibles)).arg(programa);
                c
            }
            Sandbox::Seatbelt => {
                let tapar = tapar()?;
                let (prefijo, quien) = if criterio { ("ea-criterio-", "del criterio") } else { ("ea-ejecutor-", "del agente") };
                let tmp = tempfile::Builder::new().prefix(prefijo).tempdir().map_err(|e| format!("no se pudo crear el temporal propio {quien}: {e}"))?;
                let escribibles: Vec<PathBuf> = escribibles.iter().map(|d| canonica(d)).collect();
                let (wt_c, tmp_c, ej) = (canonica(wt), canonica(tmp.path()), ejecucion.map(canonica));
                let perfil = if criterio {
                    perfil_seatbelt_criterio(&wt_c, &tmp_c, &escribibles, ej.as_deref(), &tapar)
                } else {
                    perfil_seatbelt(&wt_c, &tmp_c, &escribibles, ej.as_deref(), &tapar)
                };
                let mut c = Command::new(SANDBOX_EXEC);
                c.arg("-p").arg(perfil).arg(programa).env("TMPDIR", tmp_c);
                temporal = Some(tmp);
                c
            }
        };
        orden.args(args).current_dir(wt);
        Ok(Envoltorio { orden, _temporal: temporal })
    }
}

/// Lo que se esconde al agente y al criterio además de lo que ya no pueden
/// escribir: el directorio de ejecución de la sesión ([`dir_ejecucion`]) y
/// las rutas de `agente.tapar` (sin resolver: lo hace [`resolver_tapar`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct Ocultar<'a> {
    pub ejecucion: Option<&'a Path>,
    pub tapar: &'a [PathBuf],
}

/// La orden aislada del agente ([`Sandbox::envolver`]) o del criterio
/// ([`Sandbox::envolver_criterio`]). En macOS guarda además su temporal
/// propio, que se borra al soltarla: tiene que vivir lo que dure la corrida.
pub struct Envoltorio {
    pub orden: Command,
    _temporal: Option<tempfile::TempDir>,
}

/// Argumentos de `bwrap` (sin el programa): `/` en solo lectura, `/tmp`
/// propio, el worktree y los directorios de estado del agente escribibles.
/// El directorio de ejecución de la sesión (`$XDG_RUNTIME_DIR`), si existe,
/// queda tapado por un tmpfs vacío: ahí está la clave de sesión del panel
/// web (con ella el agente, que tiene la red abierta, podría escribir por
/// `127.0.0.1:8092`), y también los sockets de D-Bus y de `systemd --user`,
/// que no necesita. Se tapa entero y no solo `ea-web/`: así no hace falta
/// que exista ese subdirectorio (bwrap no puede crear el punto de montaje en
/// un `/` de solo lectura) y no queda ningún otro socket de la sesión a su
/// alcance.
///
/// El `<wt>/.git`, si existe (directorio en un repo normal, fichero en un
/// worktree propio), se vuelve a montar en solo lectura encima del worktree:
/// ni su configuración ni sus *hooks*, ni a dónde apunta. Si no existe,
/// bwrap no puede tapar la ruta sin crearla en el disco del usuario; por eso
/// el worktree de una orden tiene que ser la raíz de su repo
/// (`ea_bus::validar_worktree`), que siempre tiene `.git`. Un `.git` enlace
/// simbólico protege su destino, no el propio enlace.
///
/// `tapar` (`agente.tapar`, ya resuelto por [`resolver_tapar`]) se esconde
/// igual: un directorio con un tmpfs vacío y un fichero con `/dev/null`
/// encima, también antes de los bind escribibles.
pub fn argumentos_bwrap(worktree: &Path, ejecucion: Option<&Path>, tapar: &[Tapado], estado: &[PathBuf]) -> Vec<String> {
    let wt = worktree.to_string_lossy().to_string();
    let mut args: Vec<String> = ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"].map(String::from).into();
    if let Some(d) = ejecucion {
        args.push("--tmpfs".into());
        args.push(d.to_string_lossy().into());
    }
    for t in tapar {
        let ruta = t.ruta.to_string_lossy().to_string();
        if t.directorio {
            args.extend(["--tmpfs".into(), ruta]);
        } else {
            args.extend(["--ro-bind".into(), "/dev/null".into(), ruta]);
        }
    }
    // Los bind escribibles van después del tmpfs: si alguno cayera dentro del
    // directorio de ejecución, se montaría encima y seguiría visible.
    for d in estado {
        let d = d.to_string_lossy().to_string();
        args.extend(["--bind".into(), d.clone(), d]);
    }
    args.extend(["--bind".into(), wt.clone(), wt.clone()]);
    let git = worktree.join(".git");
    // `metadata` sigue un enlace: uno roto haría fallar a bwrap.
    if std::fs::metadata(&git).is_ok() {
        let git = git.to_string_lossy().to_string();
        args.extend(["--ro-bind".into(), git.clone(), git]);
    }
    for a in ["--chdir", &wt, "--unshare-pid", "--die-with-parent"] {
        args.push(a.into());
    }
    args
}

/// Literal de cadena de SBPL: entre comillas, con `\` y `"` escapados.
fn literal(ruta: &Path) -> String {
    let mut s = String::from("\"");
    for ch in ruta.to_string_lossy().chars() {
        match ch {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            c => s.push(c),
        }
    }
    s.push('"');
    s
}

/// Perfil Seatbelt del agente (puro). Las rutas deben venir canonicalizadas
/// (en macOS `/tmp` es `/private/tmp` y `/var` es `/private/var`; el perfil
/// compara contra la ruta real). En SBPL gana la última regla que case:
///
/// 1. todo permitido por defecto;
/// 2. escritura denegada en todo el disco;
/// 3. escritura permitida en el worktree, el temporal propio, los directorios
///    de estado del adaptador y los dispositivos mínimos;
/// 4. escritura denegada en `<worktree>/.git` (exista o no: tampoco se puede
///    crear uno falso en un worktree que sea subdirectorio del repo);
/// 5. lectura denegada en el directorio de ejecución (clave del panel), y
///    lectura y escritura denegadas en las rutas de `agente.tapar`;
/// 6. red denegada salvo conectar a `localhost` (llama-server), escuchar solo
///    en `localhost` y el socket de
///    `mDNSResponder`, que usa la resolución de nombres del sistema. No se
///    abre ningún otro socket unix.
///
/// Cierra las vías para lanzar código fuera del sandbox (con `allow default`
/// los hijos heredan el perfil, pero `open`, `osascript` o `launchctl` piden a
/// otro proceso, ya fuera, que lo ejecute): sin esos binarios ni los servicios
/// de LaunchServices y AppleEvents. Un agente Node normal no los usa.
const REGLAS_ESCAPE: &str = "(deny process-exec*\n  (literal \"/usr/bin/open\")\n  (literal \"/usr/bin/osascript\")\n  (literal \"/usr/bin/automator\")\n  (literal \"/usr/bin/shortcuts\")\n  (literal \"/bin/launchctl\")\n  (literal \"/usr/bin/launchctl\"))\n(deny mach-lookup\n  (global-name \"com.apple.coreservices.launchservicesd\")\n  (global-name \"com.apple.coreservices.appleevents\")\n  (global-name-prefix \"com.apple.lsd\"))\n";

/// El del agente: el del criterio ([`perfil_seatbelt_criterio`]) con la red
/// del punto 6.
pub fn perfil_seatbelt(worktree: &Path, tmp: &Path, estado: &[PathBuf], ejecucion: Option<&Path>, tapar: &[Tapado]) -> String {
    let mut p = perfil_seatbelt_criterio(worktree, tmp, estado, ejecucion, tapar);
    p.push_str("(deny network*)\n(allow network* (remote ip \"localhost:*\"))\n(allow network-bind network-inbound (local ip \"localhost:*\"))\n");
    p.push_str("(allow network* (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))\n");
    p
}

/// Perfil Seatbelt del criterio (puro): el del agente ([`perfil_seatbelt`],
/// puntos 1 a 5 y las vías de escape) sin las reglas de red; `escribibles`
/// ocupa el lugar de los directorios de estado del agente.
pub fn perfil_seatbelt_criterio(worktree: &Path, tmp: &Path, escribibles: &[PathBuf], ejecucion: Option<&Path>, tapar: &[Tapado]) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n(deny file-write* (subpath \"/\"))\n(allow file-write*\n");
    p.push_str(&format!("  (subpath {})\n  (subpath {})\n", literal(worktree), literal(tmp)));
    for d in escribibles {
        p.push_str(&format!("  (subpath {})\n", literal(d)));
    }
    p.push_str("  (literal \"/dev/null\")\n  (literal \"/dev/tty\")\n  (regex #\"^/dev/fd/\"))\n");
    let git = literal(&worktree.join(".git"));
    p.push_str(&format!("(deny file-write* (literal {git}) (subpath {git}))\n"));
    if let Some(d) = ejecucion {
        p.push_str(&format!("(deny file-read* (subpath {}))\n", literal(d)));
    }
    // `agente.tapar`: ni leer ni escribir (gana la última regla que case).
    for t in tapar {
        p.push_str(&format!("(deny file-read* file-write* (subpath {}))\n", literal(&t.ruta)));
    }
    p.push_str(REGLAS_ESCAPE);
    p
}

/// Una ruta de `agente.tapar` que existe, ya canonicalizada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tapado {
    pub ruta: PathBuf,
    /// Un directorio se tapa con un tmpfs vacío; un fichero, con `/dev/null`.
    pub directorio: bool,
}

/// Qué se tapa de verdad (puro salvo por mirar el disco): ignora las rutas
/// que no existen y rechaza, con el motivo, las que se solapan con el
/// worktree o con lo escribible (`escribibles`: el estado del agente o las
/// rutas escribibles del criterio), tanto si las contienen como si
/// coinciden o caen dentro: taparlas a medias en silencio dejaría al agente
/// sin poder trabajar, o la ruta visible por el bind escribible que va después.
pub fn resolver_tapar(tapar: &[PathBuf], worktree: &Path, escribibles: &[PathBuf]) -> Result<Vec<Tapado>, String> {
    let propios: Vec<(PathBuf, String)> = std::iter::once((canonica(worktree), "el worktree del trabajo".to_string()))
        .chain(escribibles.iter().map(|d| (canonica(d), format!("una ruta escribible del sandbox ({})", d.display()))))
        .collect();
    let mut salida: Vec<Tapado> = Vec::new();
    for t in tapar {
        let Ok(meta) = std::fs::metadata(t) else { continue };
        let ruta = canonica(t);
        if let Some((propio, que)) = propios.iter().find(|(p, _)| p.starts_with(&ruta) || ruta.starts_with(p)) {
            return Err(format!(
                "agente.tapar: la ruta {} se solapa con {que} ({}); el trabajo se rechaza en vez de taparla a medias (quítala de agente.tapar o lanza el trabajo desde otro sitio)",
                t.display(),
                propio.display()
            ));
        }
        if !salida.iter().any(|s| s.ruta == ruta) {
            salida.push(Tapado { ruta, directorio: meta.is_dir() });
        }
    }
    Ok(salida)
}

/// El directorio de ejecución de la sesión (`$XDG_RUNTIME_DIR`), si existe:
/// el que se tapa al agente y al criterio (bwrap no puede montar sobre una
/// ruta que no existe).
pub fn dir_ejecucion() -> Option<PathBuf> {
    ea_ajustes::dir_ejecucion(&ea_ajustes::Entorno::Proceso).filter(|d| d.is_dir())
}

/// Rutas reales (sin enlaces) para el perfil; si no se pueden resolver, la
/// ruta tal cual.
pub fn canonica(p: &Path) -> PathBuf {
    ea_plataforma::rutas::canonica(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_eleccion_depende_del_sistema_y_de_sandbox() {
        use Sistema::*;
        for s in [Linux, MacOs, Windows, Otro] {
            assert_eq!(Sandbox::elegir(s, false, true), Sandbox::Ninguno, "{s:?}");
        }
        assert_eq!(Sandbox::elegir(Linux, true, false), Sandbox::Bwrap);
        assert_eq!(Sandbox::elegir(MacOs, true, true), Sandbox::Seatbelt);
        assert!(matches!(Sandbox::elegir(MacOs, true, false), Sandbox::NoDisponible(m) if m.contains("sandbox-exec")));
        let Sandbox::NoDisponible(m) = Sandbox::elegir(Windows, true, true) else { panic!("Windows debe negarse") };
        assert!(m.contains("WSL2") && m.contains("sandbox = false") && m.contains("README"), "{m}");
        assert!(Sandbox::elegir(Windows, true, true).comprobar().is_err());
        assert!(Sandbox::elegir(Windows, false, true).comprobar().is_ok());
    }

    #[test]
    fn el_perfil_limita_escritura_lectura_y_red() {
        let estado = [PathBuf::from("/Users/u/.qwen")];
        let p = perfil_seatbelt(Path::new("/w/repo"), Path::new("/private/var/t/x"), &estado, Some(Path::new("/private/var/run/ea")), &[]);
        assert!(p.starts_with("(version 1)\n(allow default)\n(deny file-write* (subpath \"/\"))"));
        for esperado in [
            "(subpath \"/w/repo\")",
            "(subpath \"/private/var/t/x\")",
            "(subpath \"/Users/u/.qwen\")",
            "(literal \"/dev/null\")",
            "(regex #\"^/dev/fd/\")",
            "(deny file-read* (subpath \"/private/var/run/ea\"))",
            "(deny network*)",
            "(remote ip \"localhost:*\")",
        ] {
            assert!(p.contains(esperado), "falta {esperado}\n{p}");
        }
        // La denegación de escritura va antes de los permisos; la de red, antes de localhost.
        assert!(p.find("(deny file-write*").unwrap() < p.find("(allow file-write*").unwrap());
        assert!(p.find("(deny network*)").unwrap() < p.find("(allow network*").unwrap());
        // Una regla `local ip` que permita `network*` abriría la red entera
        // (lo comprobó la CI de macOS): solo se escucha en localhost.
        assert!(!p.contains("(allow network* (local ip"), "{p}");
        assert!(p.contains("(allow network-bind network-inbound (local ip \"localhost:*\"))"));
        // Vías de escape del sandbox cerradas, y antes de los permisos de red.
        for esperado in [
            "(deny process-exec*",
            "\"/usr/bin/open\"",
            "\"/usr/bin/osascript\"",
            "\"/bin/launchctl\"",
            "(deny mach-lookup",
            "com.apple.coreservices.launchservicesd",
            "com.apple.coreservices.appleevents",
            "(global-name-prefix \"com.apple.lsd\")",
        ] {
            assert!(p.contains(esperado), "falta {esperado}\n{p}");
        }
        assert!(p.find("(deny process-exec*").unwrap() < p.find("(deny network*)").unwrap());
        // Sin directorio de ejecución no hay nada que tapar.
        assert!(!perfil_seatbelt(Path::new("/w"), Path::new("/t"), &[], None, &[]).contains("deny file-read*"));
    }

    #[test]
    fn el_perfil_escapa_comillas_y_barras() {
        let p = perfil_seatbelt(Path::new("/w/a\"b\\c"), Path::new("/t"), &[], None, &[]);
        assert!(p.contains(r#"(subpath "/w/a\"b\\c")"#), "{p}");
        assert_eq!(literal(Path::new("a\nb")), "\"a\\nb\"");
    }

    #[cfg(unix)] // el perfil de Seatbelt (macOS) junta las rutas con `/`
    #[test]
    fn el_perfil_deja_el_git_del_worktree_de_solo_lectura() {
        // `/w/repo` no existe: la regla va igual (tampoco se puede crear un `.git`).
        let p = perfil_seatbelt(Path::new("/w/repo"), Path::new("/t"), &[], Some(Path::new("/private/var/run/ea")), &[]);
        let regla = "(deny file-write* (literal \"/w/repo/.git\") (subpath \"/w/repo/.git\"))";
        let i = p.find(regla).unwrap_or_else(|| panic!("falta {regla}\n{p}"));
        // En SBPL gana la última regla: el `deny` va después del bloque `allow`.
        assert!(p.rfind("(allow file-write*").unwrap() < i && p.find("(regex #\"^/dev/fd/\"))").unwrap() < i, "{p}");
        assert_eq!(p.matches("(allow file-write*").count(), 1, "ningún permiso de escritura detrás del deny\n{p}");
    }

    #[test]
    fn bwrap_oculta_el_directorio_de_ejecucion_y_monta_el_estado_del_agente() {
        let ejecucion = tempfile::tempdir().unwrap();
        let ej = ejecucion.path().to_string_lossy().to_string();
        let estado = [PathBuf::from("/home/u/.codex")];
        let args = argumentos_bwrap(Path::new("/w/repo"), Some(ejecucion.path()), &[], &estado);
        let pos = |par: [&str; 2]| args.windows(2).position(|w| w[0] == par[0] && w[1] == par[1]);
        let tmpfs = pos(["--tmpfs", &ej]).expect("tmpfs sobre el directorio de ejecución");
        // Encima del / de solo lectura y antes de los bind escribibles.
        assert!(pos(["--ro-bind", "/"]).unwrap() < tmpfs);
        assert!(tmpfs < pos(["--bind", "/w/repo"]).unwrap());
        assert!(tmpfs < pos(["--bind", "/home/u/.codex"]).unwrap());
        assert!(pos(["--bind", "/home/u/.qwen"]).is_none(), "solo el estado de este agente");
        assert!(args.ends_with(&["--unshare-pid".into(), "--die-with-parent".into()]));
        // Sin directorio de ejecución no hay nada que tapar.
        let sin = argumentos_bwrap(Path::new("/w/repo"), None, &[], &[]);
        assert_eq!(sin.iter().filter(|a| *a == "--tmpfs").count(), 1, "solo el de /tmp");
    }

    #[test]
    fn bwrap_monta_el_git_del_worktree_en_solo_lectura_si_existe() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().to_string_lossy().to_string();
        let git = d.path().join(".git").to_string_lossy().to_string();
        let ro_git = |args: &[String]| args.windows(3).position(|w| w[0] == "--ro-bind" && w[1] == git && w[2] == git);
        // Sin `.git` (worktree que es un subdirectorio del repo): nada que montar.
        let sin = argumentos_bwrap(d.path(), None, &[], &[]);
        assert!(ro_git(&sin).is_none(), "{sin:?}");
        // `.git` fichero (worktree propio) y directorio (repo normal): justo
        // después del bind del worktree, para quedar encima.
        std::fs::write(d.path().join(".git"), "gitdir: /r/.git/worktrees/t1\n").unwrap();
        let fichero = argumentos_bwrap(d.path(), None, &[], &[]);
        std::fs::remove_file(d.path().join(".git")).unwrap();
        std::fs::create_dir(d.path().join(".git")).unwrap();
        let directorio = argumentos_bwrap(d.path(), None, &[], &[]);
        for args in [&fichero, &directorio] {
            let bind = args.windows(3).position(|w| w[0] == "--bind" && w[1] == wt && w[2] == wt).unwrap();
            assert_eq!(ro_git(args), Some(bind + 3), "{args:?}");
            assert!(args.ends_with(&["--chdir".into(), wt.clone(), "--unshare-pid".into(), "--die-with-parent".into()]));
        }
        // Un enlace roto no se monta (bwrap fallaría).
        #[cfg(unix)]
        {
            std::fs::remove_dir(d.path().join(".git")).unwrap();
            std::os::unix::fs::symlink("/no/existe", d.path().join(".git")).unwrap();
            assert!(ro_git(&argumentos_bwrap(d.path(), None, &[], &[])).is_none());
        }
    }

    #[cfg(unix)] // el perfil de Seatbelt (macOS) junta las rutas con `/`
    #[test]
    fn el_criterio_tiene_el_perfil_de_disco_del_agente_sin_su_red() {
        let esc = [PathBuf::from("/Users/u/.cargo")];
        let ej = Some(Path::new("/private/var/run/ea"));
        let p = perfil_seatbelt_criterio(Path::new("/w/repo"), Path::new("/t"), &esc, ej, &[]);
        // Lo mismo que el del agente con esas rutas, salvo la red.
        let agente = perfil_seatbelt(Path::new("/w/repo"), Path::new("/t"), &esc, ej, &[]);
        assert!(agente.starts_with(&p), "{p}\n---\n{agente}");
        assert!(agente[p.len()..].starts_with("(deny network*)"), "{agente}");
        assert!(!p.contains("network"), "la red del criterio no cambia: {p}");
        for esperado in [
            "(deny file-write* (subpath \"/\"))",
            "(subpath \"/w/repo\")",
            "(subpath \"/t\")",
            "(subpath \"/Users/u/.cargo\")",
            "(deny file-write* (literal \"/w/repo/.git\") (subpath \"/w/repo/.git\"))",
            "(deny file-read* (subpath \"/private/var/run/ea\"))",
        ] {
            assert!(p.contains(esperado), "falta {esperado}\n{p}");
        }
        assert!(p.ends_with(REGLAS_ESCAPE), "con las vías de escape cerradas\n{p}");
        assert!(p.rfind("(allow file-write*").unwrap() < p.find("/w/repo/.git").unwrap(), "el .git, después de los permisos\n{p}");
    }

    fn argumentos(c: &Command) -> Vec<String> {
        c.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn envolver_construye_la_orden_de_cada_sandbox() {
        let wt = tempfile::tempdir().unwrap();
        let args = ["-p".to_string(), "x".to_string()];
        let estado = [PathBuf::from("/home/u/.qwen")];

        let e = Sandbox::Ninguno.envolver("qwen", &args, wt.path(), &estado, Ocultar::default()).unwrap();
        assert_eq!((e.orden.get_program(), argumentos(&e.orden)), ("qwen".as_ref(), args.to_vec()));
        assert_eq!(e.orden.get_current_dir(), Some(wt.path()));

        let e = Sandbox::Bwrap.envolver("qwen", &args, wt.path(), &estado, Ocultar::default()).unwrap();
        let mut esperado = argumentos_bwrap(wt.path(), None, &[], &estado);
        esperado.extend(["qwen".into(), "-p".into(), "x".into()]);
        assert_eq!((e.orden.get_program(), argumentos(&e.orden)), ("bwrap".as_ref(), esperado));
        assert_eq!(e.orden.get_current_dir(), Some(wt.path()));

        // Seatbelt: el perfil con el worktree real y su `.git`, y un temporal
        // propio que vive lo que el envoltorio.
        let e = Sandbox::Seatbelt.envolver("qwen", &args, wt.path(), &[], Ocultar::default()).unwrap();
        let a = argumentos(&e.orden);
        assert_eq!(e.orden.get_program(), SANDBOX_EXEC);
        assert_eq!((a[0].as_str(), &a[2..]), ("-p", &["qwen".to_string(), "-p".into(), "x".into()][..]));
        let git = canonica(wt.path()).join(".git");
        assert!(a[1].contains(&format!("(deny file-write* (literal {})", literal(&git))), "{}", a[1]);
        let tmp = e.orden.get_envs().find(|(k, _)| *k == "TMPDIR").and_then(|(_, v)| v).map(PathBuf::from).unwrap();
        assert!(tmp.is_dir() && a[1].contains(&format!("(subpath {})", literal(&tmp))));
        drop(e);
        assert!(!tmp.exists(), "el temporal se borra al soltar el envoltorio");

        let no = Sandbox::NoDisponible("sin sandbox".into());
        assert_eq!(no.envolver("qwen", &args, wt.path(), &[], Ocultar::default()).err().as_deref(), Some("sin sandbox"));
    }

    #[test]
    fn envolver_el_criterio_conserva_la_orden() {
        let wt = tempfile::tempdir().unwrap();
        let ej = tempfile::tempdir().unwrap();
        let esc = [PathBuf::from("/home/u/.cargo")];
        let criterio = |dir: Option<&Path>| {
            let mut c = Command::new("sh");
            c.args(["-c", "test -f ok.txt"]).env("A", "1").env_remove("B");
            if let Some(d) = dir {
                c.current_dir(d);
            }
            c
        };
        let entorno = |c: &Command| c.get_envs().map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string()))).collect::<Vec<_>>();

        // bwrap: los argumentos del agente, con `escribibles` en lugar de su estado.
        let e =
            Sandbox::Bwrap.envolver_criterio(criterio(Some(wt.path())), wt.path(), &esc, Ocultar { ejecucion: Some(ej.path()), ..Default::default() }).unwrap();
        let mut esperado = argumentos_bwrap(wt.path(), Some(ej.path()), &[], &esc);
        esperado.extend(["sh".into(), "-c".into(), "test -f ok.txt".into()]);
        assert_eq!((e.orden.get_program(), argumentos(&e.orden)), ("bwrap".as_ref(), esperado));
        assert_eq!(entorno(&e.orden), entorno(&criterio(None)), "mismo entorno");
        assert_eq!(e.orden.get_current_dir(), Some(wt.path()));

        // Seatbelt: el perfil del criterio con el temporal propio.
        let e = Sandbox::Seatbelt.envolver_criterio(criterio(None), wt.path(), &esc, Ocultar { ejecucion: Some(ej.path()), ..Default::default() }).unwrap();
        let a = argumentos(&e.orden);
        assert_eq!(e.orden.get_program(), SANDBOX_EXEC);
        let tmp = e.orden.get_envs().find(|(k, _)| *k == "TMPDIR").and_then(|(_, v)| v).map(PathBuf::from).unwrap();
        assert!(tmp.is_dir() && tmp.file_name().unwrap().to_string_lossy().starts_with("ea-criterio-"));
        let perfil = perfil_seatbelt_criterio(&canonica(wt.path()), &tmp, &esc, Some(&canonica(ej.path())), &[]);
        assert_eq!(a, ["-p".to_string(), perfil, "sh".into(), "-c".into(), "test -f ok.txt".into()]);
        assert_eq!(e.orden.get_current_dir(), Some(wt.path()), "sin directorio propio, el worktree");
        drop(e);
        assert!(!tmp.exists(), "el temporal se borra al soltar el envoltorio");

        // Sin sandbox, tal cual; sin sandbox posible, el motivo.
        let e = Sandbox::Ninguno
            .envolver_criterio(criterio(Some(wt.path())), wt.path(), &esc, Ocultar { ejecucion: Some(ej.path()), ..Default::default() })
            .unwrap();
        assert_eq!((e.orden.get_program(), argumentos(&e.orden)), ("sh".as_ref(), vec!["-c".to_string(), "test -f ok.txt".into()]));
        let no = Sandbox::NoDisponible("sin sandbox".into()).envolver_criterio(criterio(None), wt.path(), &[], Ocultar::default());
        assert_eq!(no.err().as_deref(), Some("sin sandbox"));
    }

    /// CE-004: Seatbelt de verdad (corre en la CI de macOS).
    #[cfg(target_os = "macos")]
    /// Spec 024 (RF-002): lo tapado va tras el `/` de solo lectura y antes de
    /// todo bind escribible; un directorio con tmpfs y un fichero con `/dev/null`.
    #[test]
    fn bwrap_tapa_directorios_con_tmpfs_y_ficheros_con_dev_null_antes_de_los_bind() {
        let tapar =
            [Tapado { ruta: PathBuf::from("/home/u/.secretos"), directorio: true }, Tapado { ruta: PathBuf::from("/home/u/.netrc"), directorio: false }];
        let estado = [PathBuf::from("/home/u/.codex")];
        let args = argumentos_bwrap(Path::new("/w/repo"), None, &tapar, &estado);
        let pos = |par: [&str; 2]| args.windows(2).position(|w| w[0] == par[0] && w[1] == par[1]);
        let dir = pos(["--tmpfs", "/home/u/.secretos"]).expect("tmpfs sobre el directorio");
        let fich = args.windows(3).position(|w| w == ["--ro-bind", "/dev/null", "/home/u/.netrc"]).expect("/dev/null sobre el fichero");
        let raiz = pos(["--ro-bind", "/"]).unwrap();
        for bind in ["/w/repo", "/home/u/.codex"] {
            let b = pos(["--bind", bind]).unwrap();
            assert!(raiz < dir && raiz < fich && dir < b && fich < b, "{bind}: {args:?}");
        }
        // Sin lista, solo el tmpfs de /tmp.
        assert_eq!(argumentos_bwrap(Path::new("/w/repo"), None, &[], &[]).iter().filter(|a| *a == "--tmpfs").count(), 1);
    }

    /// Spec 024 (RF-003): en el perfil del agente y en el del criterio, tras
    /// los permisos de escritura (gana la última regla que case).
    #[test]
    fn el_perfil_tapa_las_rutas_de_agente_tapar_tras_los_permisos() {
        let tapar = [Tapado { ruta: PathBuf::from("/Users/u/.secretos"), directorio: true }];
        let regla = "(deny file-read* file-write* (subpath \"/Users/u/.secretos\"))";
        for p in [
            perfil_seatbelt(Path::new("/w/repo"), Path::new("/t"), &[], None, &tapar),
            perfil_seatbelt_criterio(Path::new("/w/repo"), Path::new("/t"), &[], None, &tapar),
        ] {
            assert!(p.contains(regla), "{p}");
            assert!(p.find("(allow file-write*").unwrap() < p.find(regla).unwrap());
        }
        assert!(!perfil_seatbelt(Path::new("/w"), Path::new("/t"), &[], None, &[]).contains("file-read* file-write*"));
    }

    /// Spec 024 (RF-004): lo que no existe se ignora, sin repetidos, y se
    /// distingue un directorio de un fichero.
    #[test]
    fn resolver_tapar_ignora_lo_que_no_existe_y_distingue_ficheros() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("secretos");
        std::fs::create_dir(&dir).unwrap();
        let fichero = d.path().join("clave.txt");
        std::fs::write(&fichero, "x").unwrap();
        let wt = d.path().join("repo");
        std::fs::create_dir(&wt).unwrap();
        let r = resolver_tapar(&[dir.clone(), fichero.clone(), d.path().join("no-existe"), dir.clone()], &wt, &[]).unwrap();
        assert_eq!(r, [Tapado { ruta: canonica(&dir), directorio: true }, Tapado { ruta: canonica(&fichero), directorio: false }]);
        assert!(resolver_tapar(&[], &wt, &[]).unwrap().is_empty());
    }

    /// Spec 024 (RF-005): lo que contiene al worktree o a una ruta escribible
    /// (estado del agente, escribibles del criterio), coincide o cae dentro
    /// se rechaza con el motivo, también al envolver.
    #[test]
    fn resolver_tapar_rechaza_lo_que_se_solapa_con_el_worktree_o_lo_escribible() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path().join("repo");
        let estado = d.path().join(".qwen");
        std::fs::create_dir_all(wt.join("sub")).unwrap();
        std::fs::create_dir(&estado).unwrap();
        for t in [wt.clone(), d.path().to_path_buf(), wt.join("sub")] {
            let e = resolver_tapar(std::slice::from_ref(&t), &wt, &[]).unwrap_err();
            assert!(e.contains("agente.tapar") && e.contains("worktree"), "{t:?}: {e}");
        }
        let e = resolver_tapar(std::slice::from_ref(&estado), &wt, std::slice::from_ref(&estado)).unwrap_err();
        assert!(e.contains("ruta escribible"), "{e}");
        assert!(resolver_tapar(&[wt.join("nada")], &wt, &[]).unwrap().is_empty(), "lo que no existe no molesta");
        // `envolver` y `envolver_criterio` no construyen nada; sin sandbox no se mira.
        let tapar = [wt.clone()];
        let ocultar = Ocultar { ejecucion: None, tapar: &tapar };
        for s in [Sandbox::Bwrap, Sandbox::Seatbelt] {
            assert!(s.envolver("qwen", &[], &wt, &[], ocultar).err().unwrap().contains("agente.tapar"));
            assert!(s.envolver_criterio(Command::new("true"), &wt, &[], ocultar).err().unwrap().contains("agente.tapar"));
        }
        assert!(Sandbox::Ninguno.envolver("qwen", &[], &wt, &[], ocultar).is_ok());
    }

    #[test]
    fn seatbelt_impide_escribir_fuera_del_worktree() {
        use std::process::Command;
        if !Path::new(SANDBOX_EXEC).is_file() {
            // En la CI (macos-latest) tiene que estar: no se omite en silencio.
            assert!(std::env::var_os("CI").is_none(), "falta {SANDBOX_EXEC} en la CI de macOS");
            eprintln!("sin sandbox-exec: prueba omitida");
            return;
        }
        let wt = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let fuera = tempfile::tempdir().unwrap();
        let (wt_c, tmp_c, fuera_c) = (canonica(wt.path()), canonica(tmp.path()), canonica(fuera.path()));
        let run = tempfile::tempdir().unwrap();
        let run_c = canonica(run.path());
        std::fs::write(run_c.join("clave"), "secreto").unwrap();
        let tapado = tempfile::tempdir().unwrap();
        let tapado_c = canonica(tapado.path());
        std::fs::write(tapado_c.join("clave"), "secreto").unwrap();
        let t = [Tapado { ruta: tapado_c.clone(), directorio: true }];
        let perfil = perfil_seatbelt(&wt_c, &tmp_c, &[], Some(&run_c), &t);
        let sh = |orden: String| Command::new(SANDBOX_EXEC).args(["-p", &perfil, "sh", "-c"]).arg(orden).current_dir(&wt_c).output().unwrap();
        let correr = |destino: &Path| sh(format!("touch '{}'", destino.display()));
        let dentro = correr(&wt_c.join("dentro"));
        assert!(dentro.status.success() && wt_c.join("dentro").exists(), "{}", String::from_utf8_lossy(&dentro.stderr));
        assert!(correr(&tmp_c.join("tmp")).status.success(), "el temporal propio debe ser escribible");
        let f = correr(&fuera_c.join("fuera"));
        assert!(!f.status.success() && !fuera_c.join("fuera").exists(), "escribió fuera del worktree");
        // Lo de `agente.tapar` ni se lee ni se escribe (spec 024).
        let l = sh(format!("cat '{}'", tapado_c.join("clave").display()));
        assert!(!l.status.success() && l.stdout.is_empty(), "leyó una ruta tapada");
        assert!(!correr(&tapado_c.join("otro")).status.success() && !tapado_c.join("otro").exists(), "escribió en una ruta tapada");

        // `.git`: ni crear uno falso ni escribir en el del repo.
        let falso = sh("printf 'gitdir: /tmp/x' > .git".into());
        assert!(!falso.status.success() && !wt_c.join(".git").exists(), "creó un .git en el worktree");
        std::fs::create_dir(wt_c.join(".git")).unwrap();
        std::fs::write(wt_c.join(".git/config"), "[core]\n").unwrap();
        let config = sh("echo x >> .git/config".into());
        assert!(!config.status.success(), "escribió en .git/config");
        assert_eq!(std::fs::read_to_string(wt_c.join(".git/config")).unwrap(), "[core]\n");
        assert!(!sh("mkdir .git/hooks".into()).status.success() && !wt_c.join(".git/hooks").exists());
        assert!(correr(&wt_c.join("dentro2")).status.success(), "el resto del worktree sigue escribible");

        // El perfil del criterio: el mismo disco que el agente (más sus
        // rutas escribibles), sin sus reglas de red.
        let extra = tempfile::tempdir().unwrap();
        let extra_c = canonica(extra.path());
        let criterio = perfil_seatbelt_criterio(&wt_c, &tmp_c, std::slice::from_ref(&extra_c), Some(&run_c), &t);
        let c = |orden: String| Command::new(SANDBOX_EXEC).args(["-p", &criterio, "sh", "-c"]).arg(orden).current_dir(&wt_c).output().unwrap();
        let l = c(format!("cat '{}'", run_c.join("clave").display()));
        assert!(!l.status.success() && l.stdout.is_empty(), "el criterio leyó el directorio de ejecución");
        let f = c(format!("touch '{}'", fuera_c.join("criterio").display()));
        assert!(!f.status.success() && !fuera_c.join("criterio").exists(), "el criterio escribió fuera del worktree");
        assert!(c(format!("touch '{}'", extra_c.join("criterio").display())).status.success() && extra_c.join("criterio").exists());
        let l = c(format!("cat '{}'", tapado_c.join("clave").display()));
        assert!(!l.status.success() && l.stdout.is_empty(), "el criterio leyó una ruta tapada");
        assert!(c("touch criterio-dentro".into()).status.success() && wt_c.join("criterio-dentro").exists());
        assert!(!c("echo x >> .git/config".into()).status.success(), "el criterio escribió en .git/config");

        // El directorio de ejecución (clave del panel) no se puede leer.
        let l = sh(format!("cat '{}'", run_c.join("clave").display()));
        assert!(!l.status.success() && l.stdout.is_empty(), "leyó el directorio de ejecución");

        // Vías de escape: lanzar código fuera del sandbox falla; lo normal sigue.
        for orden in ["/usr/bin/open -a Calculator", "/usr/bin/osascript -e 'return 1'", "/bin/launchctl submit -l x -- /usr/bin/true"] {
            let o = sh(orden.into());
            assert!(!o.status.success(), "`{orden}` no debería poder dentro del sandbox");
        }
        let ok = sh("echo ok".into());
        assert!(ok.status.success() && String::from_utf8_lossy(&ok.stdout).trim() == "ok");
        if Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success()) {
            let n = sh(format!("node -e 'require(\"fs\").writeFileSync(\"{}/x\",\"1\")'", tmp_c.display()));
            assert!(n.status.success() && tmp_c.join("x").exists(), "node debe poder escribir en su temporal: {}", String::from_utf8_lossy(&n.stderr));
        } else {
            eprintln!("sin node: comprobación de Node omitida");
        }

        // Red: solo localhost.
        let oyente = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let puerto = oyente.local_addr().unwrap().port();
        let local = sh(format!("/usr/bin/nc -z -w 3 127.0.0.1 {puerto}"));
        assert!(local.status.success(), "localhost debe poder: {}", String::from_utf8_lossy(&local.stderr));
        let fuera_red = sh("/usr/bin/nc -z -w 3 1.1.1.1 80".into());
        assert!(!fuera_red.status.success(), "salió a la red");
    }

    #[test]
    fn la_info_del_latido_lleva_el_motivo_si_no_hay_sandbox() {
        use Sistema::*;
        let ok = Sandbox::elegir(Linux, true, false).info(Linux);
        assert!(ok.disponible && ok.sandbox == "bwrap" && ok.motivo.is_none());
        let no = Sandbox::elegir(Windows, true, false).info(Windows);
        assert!(!no.disponible && no.sistema == "Windows" && no.motivo.is_some());
        assert_eq!(Sandbox::elegir(Windows, false, false).info(Windows).sandbox, "ninguno");
    }
}
