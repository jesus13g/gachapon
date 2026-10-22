//! Aislamiento del ejecutor por sistema (spec 020, decisión 2; art. V y VIII).
//!
//! - Linux: `bwrap` (los argumentos viven en [`crate::agentes::argumentos_bwrap`]).
//! - macOS: `sandbox-exec` con un perfil Seatbelt (SBPL) equivalente, generado
//!   por [`perfil_seatbelt`]: escritura solo en el worktree, un temporal
//!   propio y los directorios de estado del adaptador; el directorio de
//!   ejecución, ilegible; red solo hacia `localhost`.
//! - Windows (y cualquier otro): no hay equivalente. Con `sandbox = true` el
//!   ejecutor **no arranca** y dice por qué; no se relaja en silencio.

use std::path::{Path, PathBuf};

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

    /// `Err(motivo)` si el ejecutor no puede arrancar con este aislamiento.
    pub fn comprobar(&self) -> Result<(), String> {
        match self {
            Sandbox::NoDisponible(m) => Err(m.clone()),
            _ => Ok(()),
        }
    }
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
/// 4. lectura denegada en el directorio de ejecución (clave del panel);
/// 5. red denegada salvo `localhost` (llama-server) y el socket de
///    `mDNSResponder`, que usa la resolución de nombres del sistema. No se
///    abre ningún otro socket unix.
pub fn perfil_seatbelt(worktree: &Path, tmp: &Path, estado: &[PathBuf], ejecucion: Option<&Path>) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n(deny file-write* (subpath \"/\"))\n(allow file-write*\n");
    p.push_str(&format!("  (subpath {})\n  (subpath {})\n", literal(worktree), literal(tmp)));
    for d in estado {
        p.push_str(&format!("  (subpath {})\n", literal(d)));
    }
    p.push_str("  (literal \"/dev/null\")\n  (literal \"/dev/tty\")\n  (regex #\"^/dev/fd/\"))\n");
    if let Some(d) = ejecucion {
        p.push_str(&format!("(deny file-read* (subpath {}))\n", literal(d)));
    }
    p.push_str("(deny network*)\n(allow network* (remote ip \"localhost:*\"))\n(allow network* (local ip \"localhost:*\"))\n");
    p.push_str("(allow network* (remote unix-socket (path-literal \"/private/var/run/mDNSResponder\")))\n");
    p
}

/// Rutas reales (sin enlaces) para el perfil; si no se pueden resolver, la
/// ruta tal cual.
pub fn canonica(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
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
        let p = perfil_seatbelt(Path::new("/w/repo"), Path::new("/private/var/t/x"), &estado, Some(Path::new("/private/var/run/ea")));
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
        // Sin directorio de ejecución no hay nada que tapar.
        assert!(!perfil_seatbelt(Path::new("/w"), Path::new("/t"), &[], None).contains("deny file-read*"));
    }

    #[test]
    fn el_perfil_escapa_comillas_y_barras() {
        let p = perfil_seatbelt(Path::new("/w/a\"b\\c"), Path::new("/t"), &[], None);
        assert!(p.contains(r#"(subpath "/w/a\"b\\c")"#), "{p}");
        assert_eq!(literal(Path::new("a\nb")), "\"a\\nb\"");
    }

    /// CE-004: Seatbelt de verdad (corre en la CI de macOS).
    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_impide_escribir_fuera_del_worktree() {
        use std::process::Command;
        if !Path::new(SANDBOX_EXEC).is_file() {
            eprintln!("sin sandbox-exec: prueba omitida");
            return;
        }
        let wt = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let fuera = tempfile::tempdir().unwrap();
        let (wt_c, tmp_c, fuera_c) = (canonica(wt.path()), canonica(tmp.path()), canonica(fuera.path()));
        let perfil = perfil_seatbelt(&wt_c, &tmp_c, &[], None);
        let correr = |destino: &Path| {
            Command::new(SANDBOX_EXEC)
                .args(["-p", &perfil, "sh", "-c"])
                .arg(format!("touch '{}'", destino.display()))
                .current_dir(&wt_c)
                .output()
                .unwrap()
        };
        let dentro = correr(&wt_c.join("dentro"));
        assert!(dentro.status.success() && wt_c.join("dentro").exists(), "{}", String::from_utf8_lossy(&dentro.stderr));
        assert!(correr(&tmp_c.join("tmp")).status.success(), "el temporal propio debe ser escribible");
        let f = correr(&fuera_c.join("fuera"));
        assert!(!f.status.success() && !fuera_c.join("fuera").exists(), "escribió fuera del worktree");
    }
}
