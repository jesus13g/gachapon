//! Procesos: comprobar si vive, lanzar en grupo propio y terminar árboles.

use std::io;
use std::process::Command;

/// ¿Existe un proceso con este pid? Unix: `kill(pid, 0)` (un `EPERM` también
/// cuenta como vivo: existe pero es de otro usuario). Windows: `OpenProcess`
/// y `GetExitCodeProcess == STILL_ACTIVE`. Un zombi sin recoger cuenta como
/// vivo en Unix.
pub fn vive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let Ok(p) = libc::pid_t::try_from(pid) else { return false };
        // SAFETY: señal 0 no envía nada; solo comprueba la existencia.
        if unsafe { libc::kill(p, 0) } == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: llamadas FFI con argumentos válidos; el handle se cierra siempre.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut codigo: u32 = 0;
            let ok = GetExitCodeProcess(h, &mut codigo) != 0;
            CloseHandle(h);
            ok && codigo == STILL_ACTIVE as u32
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Nombre del ejecutable del proceso `pid` (sin ruta), si se puede saber.
/// Linux: `/proc/<pid>/exe` y, si no, `/proc/<pid>/comm`. macOS: `ps -o comm=`.
/// Windows: `tasklist`. `None` si el proceso no existe o no se pudo leer.
pub fn nombre(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Some(n) = std::fs::read_link(format!("/proc/{pid}/exe")).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())) {
            return Some(n.trim_end_matches(" (deleted)").to_string());
        }
        let n = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(n.trim().to_string()).filter(|n| !n.is_empty())
    }
    #[cfg(target_os = "macos")]
    {
        let sal = Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().ok()?;
        let t = String::from_utf8_lossy(&sal.stdout).trim().to_string();
        let base = t.rsplit('/').next().unwrap_or("").to_string();
        Some(base).filter(|n| !n.is_empty())
    }
    #[cfg(windows)]
    {
        let sal = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output().ok()?;
        let t = String::from_utf8_lossy(&sal.stdout);
        let n = t.lines().next()?.trim().strip_prefix('"')?.split('"').next()?.to_string();
        Some(n).filter(|n| !n.is_empty())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        None
    }
}

/// Prepara la orden para que el hijo sea cabeza de un grupo propio (Unix
/// `process_group(0)`) o de un grupo de procesos nuevo (Windows
/// `CREATE_NEW_PROCESS_GROUP`). Llamar antes de `spawn`.
pub fn preparar_grupo(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// Prepara la orden para que no abra ninguna ventana de consola (Windows
/// `CREATE_NO_WINDOW`, p. ej. `wsl.exe` lanzado desde la app de escritorio).
/// En los demás sistemas no hace nada. Llamar antes de `spawn`.
pub fn sin_ventana(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Gemelo de [`preparar_grupo`] para `tokio::process::Command`.
#[cfg(feature = "tokio")]
pub fn preparar_grupo_tokio(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// Un hijo y todo lo que lance, para terminarlos juntos.
///
/// - Unix: el hijo debe haberse lanzado con [`preparar_grupo`] (es cabeza de
///   su grupo, `pgid == pid`); las señales van al grupo entero.
/// - Windows: se crea un Job Object con `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
///   y se le asigna el proceso. **Carrera:** los nietos que el hijo lance antes
///   de que `adoptar` lo asigne al trabajo no quedan dentro; para minimizarlo,
///   adoptar inmediatamente después de `spawn`.
///
/// Al soltar el `Arbol` en Windows se cierra el handle del Job Object y, por
/// `KILL_ON_JOB_CLOSE`, **Windows mata todo lo que quede dentro**. En Unix
/// soltarlo no hace nada.
#[derive(Debug)]
pub struct Arbol {
    #[cfg(unix)]
    pgid: libc::pid_t,
    #[cfg(windows)]
    job: ManejadorJob,
    #[cfg(not(any(unix, windows)))]
    _pid: u32,
}

#[cfg(windows)]
#[derive(Debug)]
struct ManejadorJob(windows_sys::Win32::Foundation::HANDLE);
// SAFETY: un handle de Job Object se puede usar desde cualquier hilo.
#[cfg(windows)]
unsafe impl Send for ManejadorJob {}
#[cfg(windows)]
unsafe impl Sync for ManejadorJob {}

impl Arbol {
    /// Adopta el proceso `pid` (recién lanzado) como raíz del árbol.
    pub fn adoptar(pid: u32) -> io::Result<Arbol> {
        #[cfg(unix)]
        {
            let pgid = libc::pid_t::try_from(pid).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
            Ok(Arbol { pgid })
        }
        #[cfg(windows)]
        {
            use std::mem::{size_of, zeroed};
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JobObjectExtendedLimitInformation, SetInformationJobObject,
            };
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
            // SAFETY: llamadas FFI con punteros a locales válidos; los handles
            // se cierran en cada camino de error.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    let e = io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(e);
                }
                let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if h.is_null() {
                    let e = io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(e);
                }
                let asignado = AssignProcessToJobObject(job, h);
                let e = io::Error::last_os_error();
                CloseHandle(h);
                if asignado == 0 {
                    CloseHandle(job);
                    return Err(e);
                }
                Ok(Arbol { job: ManejadorJob(job) })
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Arbol { _pid: pid })
        }
    }

    /// ¿Existe una forma suave de pedir terminar ([`Arbol::terminar_suave`])?
    /// Solo en Unix; en Windows quien llama puede saltarse el margen de espera.
    pub const fn admite_terminar_suave() -> bool {
        cfg!(unix)
    }

    /// Pide terminar con educación: SIGTERM al grupo en Unix. En Windows no
    /// hay equivalente y no hace nada (usar [`Arbol::matar`] tras el margen).
    pub fn terminar_suave(&self) {
        #[cfg(unix)]
        {
            // SAFETY: `kill` con pid negativo señala al grupo; sin punteros.
            unsafe { libc::kill(-self.pgid, libc::SIGTERM) };
        }
    }

    /// Mata el árbol entero: SIGKILL al grupo en Unix, `TerminateJobObject` en
    /// Windows. Idempotente y sin error si ya no queda nada.
    pub fn matar(&self) {
        #[cfg(unix)]
        {
            // SAFETY: igual que en `terminar_suave`.
            unsafe { libc::kill(-self.pgid, libc::SIGKILL) };
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::JobObjects::TerminateJobObject;
            // SAFETY: el handle es válido mientras `self` viva.
            unsafe { TerminateJobObject(self.job.0, 1) };
        }
    }
}

#[cfg(windows)]
impl Drop for Arbol {
    fn drop(&mut self) {
        // SAFETY: el handle es nuestro y se cierra una sola vez. Con
        // KILL_ON_JOB_CLOSE, cerrarlo mata los procesos que sigan dentro.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job.0) };
    }
}

/// Restaura SIGPIPE a su acción por defecto (Unix) para que `ea … | head`
/// termine en silencio en vez de provocar un pánico de Rust por `EPIPE`.
/// En Windows no hace nada.
pub fn ignorar_sigpipe_por_defecto() {
    #[cfg(unix)]
    {
        // SAFETY: restaurar la acción por defecto de una señal es seguro.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_propio_proceso_vive_y_un_pid_inexistente_no() {
        assert!(vive(std::process::id()));
        assert!(!vive(0));
        // Un hijo ya recogido no vive.
        let mut h = Command::new(if cfg!(windows) { "cmd" } else { "true" });
        if cfg!(windows) {
            h.args(["/C", "exit 0"]);
        }
        let mut hijo = h.spawn().unwrap();
        let pid = hijo.id();
        hijo.wait().unwrap();
        assert!(!vive(pid));
    }

    #[test]
    fn una_orden_sin_ventana_sigue_ejecutandose() {
        let mut h = Command::new(if cfg!(windows) { "cmd" } else { "true" });
        if cfg!(windows) {
            h.args(["/C", "exit 0"]);
        }
        sin_ventana(&mut h);
        assert!(h.status().unwrap().success());
    }

    #[cfg(unix)]
    fn muerto(pid: u32) -> bool {
        if !vive(pid) {
            return true;
        }
        // Un zombi sin recoger (sin init que lo recoja) también está muerto.
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(')').next().is_some_and(|r| r.trim_start().starts_with('Z')))
    }

    #[cfg(unix)]
    #[test]
    fn el_arbol_mata_al_hijo_y_al_nieto() {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 600 & echo $!; wait"]).stdout(Stdio::piped());
        preparar_grupo(&mut cmd);
        let mut hijo = cmd.spawn().unwrap();
        let arbol = Arbol::adoptar(hijo.id()).unwrap();
        let mut linea = String::new();
        BufReader::new(hijo.stdout.take().unwrap()).read_line(&mut linea).unwrap();
        let nieto: u32 = linea.trim().parse().unwrap();
        assert!(vive(nieto) && vive(hijo.id()));
        arbol.matar();
        hijo.wait().unwrap();
        for _ in 0..100 {
            if muerto(nieto) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(muerto(nieto), "el nieto sigue vivo");
    }

    #[test]
    fn terminar_suave_solo_en_unix() {
        assert_eq!(Arbol::admite_terminar_suave(), cfg!(unix));
    }

    #[cfg(unix)]
    #[test]
    fn terminar_suave_manda_sigterm_al_grupo() {
        let mut cmd = Command::new("sleep");
        cmd.arg("600");
        preparar_grupo(&mut cmd);
        let mut hijo = cmd.spawn().unwrap();
        let arbol = Arbol::adoptar(hijo.id()).unwrap();
        arbol.terminar_suave();
        let st = hijo.wait().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(st.signal(), Some(libc::SIGTERM));
    }
}
