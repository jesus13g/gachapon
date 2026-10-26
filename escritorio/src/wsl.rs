//! Modo WSL (spec 023, decisión 9): en Windows con `escritorio.wsl`, los
//! servicios corren dentro de esa distribución de WSL2. El panel se ve por
//! el reenvío de `localhost` de WSL2; la clave de sesión se lee de dentro con
//! `wsl.exe` (por la salida estándar, nunca en argv) y, mientras la app vive,
//! un `sleep infinity` sin ventana evita que WSL apague la distribución por
//! inactividad. La app no arranca ni reinicia servicios: los arranca
//! `systemd` dentro de WSL.

use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::clave;

/// La distribución del modo WSL, si aplica: solo en Windows y con un nombre
/// válido en `escritorio.wsl`.
pub fn distro_activa(ajuste: &str, es_windows: bool) -> Option<String> {
    let d = ajuste.trim();
    let valida = !d.is_empty() && d.len() <= 64 && d.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    (es_windows && valida).then(|| d.to_string())
}

/// Guion `sh` que escribe la clave por la salida estándar: el mismo fichero
/// que `ea_ajustes::fichero_clave_web` dentro del directorio de ejecución
/// de Linux (`$XDG_RUNTIME_DIR` o `/run/user/<uid>`).
pub fn guion_clave() -> String {
    format!(
        "d=\"${{XDG_RUNTIME_DIR:-/run/user/$(id -u)}}\"; cat \"$d/{}/{}\"",
        ea_ajustes::DIR_WEB,
        ea_ajustes::FICHERO_CLAVE
    )
}

/// `wsl.exe` para leer la clave.
pub fn argv_clave(distro: &str) -> Vec<String> {
    ["wsl.exe", "-d", distro, "--exec", "sh", "-c"].iter().map(|s| s.to_string()).chain([guion_clave()]).collect()
}

/// `wsl.exe` que mantiene viva la distribución.
pub fn argv_viva(distro: &str) -> Vec<String> {
    ["wsl.exe", "-d", distro, "--exec", "sleep", "infinity"].iter().map(|s| s.to_string()).collect()
}

/// La clave de la salida de `wsl.exe` (sin NUL ni espacios), si es válida.
pub fn clave_de_salida(salida: &[u8]) -> Option<String> {
    let t: String = String::from_utf8_lossy(salida).chars().filter(|c| *c != '\0').collect();
    let c = t.trim();
    clave::es_valida(c).then(|| c.to_string())
}

fn orden(argv: &[String]) -> Command {
    let mut c = Command::new(&argv[0]);
    c.args(&argv[1..]);
    ea_plataforma::proceso::sin_ventana(&mut c);
    c
}

/// Lee la clave de la distribución. `None` si WSL o la clave no están.
pub fn leer_clave(distro: &str) -> Option<String> {
    let s = orden(&argv_clave(distro)).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !s.status.success() {
        return None;
    }
    clave_de_salida(&s.stdout)
}

/// El `sleep infinity` en marcha (uno por proceso), cuándo se lanzó, cuántas
/// veces seguidas ha muerto enseguida y desde cuándo se puede relanzar.
struct Viva {
    hijo: Option<Child>,
    lanzado: Option<Instant>,
    fallos: u32,
    no_antes: Option<Instant>,
}

static VIVA: Mutex<Viva> = Mutex::new(Viva { hijo: None, lanzado: None, fallos: 0, no_antes: None });

/// Un `sleep` que muere antes de esto cuenta como fallo (distribución
/// borrada, WSL roto): no se relanza cada 2 s sin fin.
const VIDA_MINIMA: Duration = Duration::from_secs(10);

/// Espera antes de relanzar tras `fallos` muertes rápidas seguidas: 0 al
/// principio, luego 4 s, 8 s… hasta 5 min.
pub fn espera_reintento(fallos: u32) -> Duration {
    if fallos == 0 { Duration::ZERO } else { Duration::from_secs((2u64 << fallos.min(8)).min(300)) }
}

/// Arranca el proceso que mantiene viva la distribución (si no lo está ya).
pub fn mantener_viva(distro: &str) {
    let mut g = VIVA.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(h) = g.hijo.as_mut() {
        if matches!(h.try_wait(), Ok(None)) {
            return;
        }
        let rapida = g.lanzado.is_some_and(|t| t.elapsed() < VIDA_MINIMA);
        g.fallos = if rapida { g.fallos + 1 } else { 0 };
        g.no_antes = Some(Instant::now() + espera_reintento(g.fallos));
        g.hijo = None;
    }
    if g.no_antes.is_some_and(|t| Instant::now() < t) {
        return;
    }
    match orden(&argv_viva(distro)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(h) => {
            g.hijo = Some(h);
            g.lanzado = Some(Instant::now());
        }
        Err(e) => eprintln!("ea-escritorio: no se pudo mantener viva la distribución {distro} de WSL: {e}"),
    }
}

/// Termina el proceso que mantiene viva la distribución (al salir).
pub fn soltar() {
    let mut g = VIVA.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(mut h) = g.hijo.take() {
        let _ = h.kill();
        let _ = h.wait();
    }
}

/// Cada cuántas vueltas del vigía se relee la clave por `wsl.exe` (lanzar un
/// proceso cada 2 s sería caro): 5 × 2 s = 10 s.
pub const VUELTAS_POR_LECTURA: u32 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    const BUENA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn los_reintentos_se_espacian_hasta_cinco_minutos() {
        assert_eq!(espera_reintento(0), Duration::ZERO);
        assert_eq!(espera_reintento(1), Duration::from_secs(4));
        assert_eq!(espera_reintento(2), Duration::from_secs(8));
        assert_eq!(espera_reintento(30), Duration::from_secs(300));
    }

    #[test]
    fn solo_en_windows_y_con_un_nombre_valido() {
        assert_eq!(distro_activa("Ubuntu", true).as_deref(), Some("Ubuntu"));
        assert_eq!(distro_activa(" Ubuntu-24.04 ", true).as_deref(), Some("Ubuntu-24.04"));
        assert_eq!(distro_activa("Ubuntu", false), None);
        assert_eq!(distro_activa("", true), None);
        assert_eq!(distro_activa("a b", true), None);
        assert_eq!(distro_activa("x\";calc", true), None);
    }

    #[test]
    fn la_clave_se_pide_por_la_salida_y_nunca_va_en_argv() {
        let a = argv_clave("Ubuntu");
        assert_eq!(&a[..6], &["wsl.exe", "-d", "Ubuntu", "--exec", "sh", "-c"]);
        assert_eq!(a.len(), 7);
        assert!(a[6].contains("${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"));
        assert!(a[6].ends_with("cat \"$d/ea-web/clave\""));
        assert_eq!(argv_viva("Ubuntu"), vec!["wsl.exe", "-d", "Ubuntu", "--exec", "sleep", "infinity"]);
    }

    #[test]
    fn el_guion_lee_el_fichero_de_la_clave_de_verdad() {
        // En Unix, el mismo guion con un XDG_RUNTIME_DIR temporal.
        let Some(sh) = ea_plataforma::rutas::buscar_en_path("sh") else { return };
        if cfg!(windows) {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("ea-web")).unwrap();
        std::fs::write(d.path().join("ea-web/clave"), format!("{BUENA}\n")).unwrap();
        let s = Command::new(sh).args(["-c", &guion_clave()]).env("XDG_RUNTIME_DIR", d.path()).output().unwrap();
        assert!(s.status.success());
        assert_eq!(clave_de_salida(&s.stdout).as_deref(), Some(BUENA));
    }

    #[test]
    fn la_salida_se_limpia_y_se_valida() {
        assert_eq!(clave_de_salida(format!("{BUENA}\r\n").as_bytes()).as_deref(), Some(BUENA));
        assert_eq!(clave_de_salida(b""), None);
        assert_eq!(clave_de_salida(b"cat: no such file"), None);
        let con_nul: Vec<u8> = BUENA.bytes().flat_map(|b| [b, 0]).collect();
        assert_eq!(clave_de_salida(&con_nul).as_deref(), Some(BUENA));
    }
}
