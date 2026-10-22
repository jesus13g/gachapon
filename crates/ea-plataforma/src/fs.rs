//! Ficheros: permisos privados, enlaces, renombrado sin pisar, ejecutables.

use std::fs::{self, Metadata, OpenOptions};
use std::io;
use std::path::Path;

/// Crea el directorio (y los padres) y, en Unix, lo deja en 0700.
pub fn crear_dir_privado(ruta: &Path) -> io::Result<()> {
    fs::create_dir_all(ruta)?;
    poner_privado(ruta)
}

/// `OpenOptions` que, al crear un fichero, lo hace con 0600 en Unix. En
/// Windows son las opciones normales (el perfil del usuario ya protege).
pub fn opciones_privadas() -> OpenOptions {
    let mut o = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// Deja el fichero (0600) o directorio (0700) solo para el usuario en Unix.
/// En Windows no hace nada.
pub fn poner_privado(ruta: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let modo = if fs::metadata(ruta)?.is_dir() { 0o700 } else { 0o600 };
        poner_permisos_unix(ruta, modo)
    }
    #[cfg(not(unix))]
    {
        let _ = ruta;
        Ok(())
    }
}

/// ¿Es privado (sin permisos para grupo ni otros)? En Windows devuelve
/// siempre `true`: **limitación conocida**, no se inspecciona la ACL; dentro
/// del perfil del usuario la ACL heredada ya impide el acceso a otros.
pub fn es_privado(ruta: &Path) -> io::Result<bool> {
    let meta = fs::metadata(ruta)?;
    Ok(permisos_unix(&meta).is_none_or(|m| m & 0o077 == 0))
}

/// Bits de permiso Unix (`mode & 0o7777`); `None` fuera de Unix.
pub fn permisos_unix(meta: &Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

/// Pone los bits de permiso Unix. No hace nada fuera de Unix.
pub fn poner_permisos_unix(ruta: &Path, modo: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(ruta, fs::Permissions::from_mode(modo))
    }
    #[cfg(not(unix))]
    {
        let _ = (ruta, modo);
        Ok(())
    }
}

/// Cómo quedó un [`enlazar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipoEnlace {
    /// Un enlace simbólico.
    Simbolico,
    /// Una copia (Windows sin privilegio para crear enlaces simbólicos).
    Copia,
}

/// Hace que `enlace` apunte a `apuntado`. En Unix, un enlace simbólico. En
/// Windows, `symlink_dir`/`symlink_file` según el destino y, si falla
/// (típicamente por falta de privilegio o modo desarrollador), una copia
/// recursiva: la copia **no** sigue los cambios del original.
pub fn enlazar(apuntado: &Path, enlace: &Path) -> io::Result<TipoEnlace> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(apuntado, enlace)?;
        Ok(TipoEnlace::Simbolico)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{symlink_dir, symlink_file};
        let real = if apuntado.is_absolute() { apuntado.to_path_buf() } else { enlace.parent().unwrap_or(Path::new("")).join(apuntado) };
        let es_dir = fs::metadata(&real).is_ok_and(|m| m.is_dir());
        let r = if es_dir { symlink_dir(apuntado, enlace) } else { symlink_file(apuntado, enlace) };
        match r {
            Ok(()) => Ok(TipoEnlace::Simbolico),
            Err(_) => {
                copiar_recursivo(&real, enlace)?;
                Ok(TipoEnlace::Copia)
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (apuntado, enlace);
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(windows)]
fn copiar_recursivo(origen: &Path, destino: &Path) -> io::Result<()> {
    if fs::metadata(origen)?.is_dir() {
        fs::create_dir_all(destino)?;
        for e in fs::read_dir(origen)? {
            let e = e?;
            copiar_recursivo(&e.path(), &destino.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(origen, destino).map(|_| ())
    }
}

/// Por qué falló un [`renombrar_sin_pisar`].
#[derive(Debug)]
pub enum ErrorRenombrar {
    /// El destino ya existe; no se ha tocado.
    Existe,
    /// Origen y destino están en sistemas de ficheros distintos.
    OtroDispositivo,
    /// El sistema (o el sistema de ficheros) no puede hacerlo atómicamente
    /// sin pisar; el texto dice por qué.
    NoSoportado(String),
    /// Cualquier otro error de E/S.
    Io(io::Error),
}

impl std::fmt::Display for ErrorRenombrar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorRenombrar::Existe => write!(f, "el destino ya existe"),
            ErrorRenombrar::OtroDispositivo => write!(f, "origen y destino están en sistemas de ficheros distintos"),
            ErrorRenombrar::NoSoportado(m) => write!(f, "{m}"),
            ErrorRenombrar::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ErrorRenombrar {}

impl From<io::Error> for ErrorRenombrar {
    fn from(e: io::Error) -> Self {
        ErrorRenombrar::Io(e)
    }
}

/// Mueve `origen` a `destino` sin pisar nada y de forma atómica frente a un
/// destino creado entre la comprobación y el movimiento: `renameat2` con
/// `RENAME_NOREPLACE` en Linux, `renamex_np` con `RENAME_EXCL` en macOS,
/// `MoveFileExW` sin `MOVEFILE_REPLACE_EXISTING` en Windows. En otros Unix
/// devuelve `NoSoportado`.
pub fn renombrar_sin_pisar(origen: &Path, destino: &Path) -> Result<(), ErrorRenombrar> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c = |p: &Path| CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL));
        let (o, d) = (c(origen)?, c(destino)?);
        // SAFETY: dos cadenas C válidas y terminadas en NUL que viven toda la llamada.
        #[cfg(target_os = "linux")]
        let r = unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, o.as_ptr(), libc::AT_FDCWD, d.as_ptr(), libc::RENAME_NOREPLACE) as i64 };
        // SAFETY: igual que arriba.
        #[cfg(target_os = "macos")]
        let r = unsafe { libc::renamex_np(o.as_ptr(), d.as_ptr(), libc::RENAME_EXCL) as i64 };
        if r == 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        Err(match e.raw_os_error() {
            Some(libc::EXDEV) => ErrorRenombrar::OtroDispositivo,
            Some(libc::EEXIST | libc::ENOTEMPTY) => ErrorRenombrar::Existe,
            Some(libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP) => ErrorRenombrar::NoSoportado(format!(
                "el sistema de ficheros no admite mover sin riesgo de pisar algo (RENAME_NOREPLACE/RENAME_EXCL): {e}"
            )),
            _ => ErrorRenombrar::Io(e),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
        const ERROR_FILE_EXISTS: i32 = 80;
        const ERROR_NOT_SAME_DEVICE: i32 = 17;
        const ERROR_ALREADY_EXISTS: i32 = 183;
        let ancho = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
        if ancho(origen).len() == 1 || ancho(destino).len() == 1 {
            return Err(ErrorRenombrar::Io(io::Error::from(io::ErrorKind::InvalidInput)));
        }
        let (o, d) = (ancho(origen), ancho(destino));
        // SAFETY: dos cadenas anchas terminadas en NUL que viven toda la llamada.
        let ok = unsafe { MoveFileExW(o.as_ptr(), d.as_ptr(), 0) };
        if ok != 0 {
            return Ok(());
        }
        let e = io::Error::last_os_error();
        Err(match e.raw_os_error() {
            Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS) => ErrorRenombrar::Existe,
            Some(ERROR_NOT_SAME_DEVICE) => ErrorRenombrar::OtroDispositivo,
            // Mover un directorio sobre otro existente da «acceso denegado».
            Some(5) if fs::symlink_metadata(destino).is_ok() => ErrorRenombrar::Existe,
            _ => ErrorRenombrar::Io(e),
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (origen, destino);
        Err(ErrorRenombrar::NoSoportado("este sistema no ofrece un renombrado atómico sin pisar".into()))
    }
}

/// ¿Es un fichero que se puede ejecutar? Unix: fichero con algún bit `x`.
/// Windows: fichero con extensión de `PATHEXT` (o `.exe/.cmd/.bat/.com` si no
/// está definida).
pub fn es_ejecutable(ruta: &Path) -> bool {
    let Ok(meta) = fs::metadata(ruta) else { return false };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        permisos_unix(&meta).is_some_and(|m| m & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        let pathext = std::env::var_os("PATHEXT");
        let exts = crate::rutas::extensiones_ejecutables(pathext.as_deref());
        ruta.extension().is_some_and(|e| {
            let e = format!(".{}", e.to_string_lossy().to_ascii_lowercase());
            exts.iter().any(|x| *x == e)
        })
    }
}

/// `fsync` del directorio para que un renombrado o creación sobreviva a un
/// corte de luz. Mejor esfuerzo: errores ignorados; no hace nada en Windows.
pub fn sincronizar_dir(ruta: &Path) {
    #[cfg(unix)]
    {
        if let Ok(f) = fs::File::open(ruta) {
            let _ = f.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ruta;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renombrar_sin_pisar_mueve_si_no_existe() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        fs::write(&a, "x").unwrap();
        renombrar_sin_pisar(&a, &b).unwrap();
        assert!(!a.exists() && b.exists());
    }

    #[test]
    fn renombrar_sin_pisar_respeta_el_destino() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        fs::write(&a, "nuevo").unwrap();
        fs::write(&b, "viejo").unwrap();
        assert!(matches!(renombrar_sin_pisar(&a, &b), Err(ErrorRenombrar::Existe)));
        assert_eq!(fs::read_to_string(&b).unwrap(), "viejo");
        assert!(a.exists());
    }

    #[test]
    fn renombrar_sin_pisar_directorios() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("da"), d.path().join("db"));
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        assert!(matches!(renombrar_sin_pisar(&a, &b), Err(ErrorRenombrar::Existe)));
    }

    #[test]
    fn privado_y_permisos() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("x/y");
        crear_dir_privado(&dir).unwrap();
        assert!(es_privado(&dir).unwrap());
        let f = dir.join("f");
        {
            use std::io::Write;
            opciones_privadas().write(true).create_new(true).open(&f).unwrap().write_all(b"s").unwrap();
        }
        assert!(es_privado(&f).unwrap());
        #[cfg(unix)]
        {
            poner_permisos_unix(&f, 0o644).unwrap();
            assert!(!es_privado(&f).unwrap());
            assert_eq!(permisos_unix(&fs::metadata(&f).unwrap()), Some(0o644));
            poner_privado(&f).unwrap();
            assert!(es_privado(&f).unwrap());
        }
        sincronizar_dir(&dir);
    }

    #[test]
    fn enlazar_apunta_al_destino() {
        let d = tempfile::tempdir().unwrap();
        let (a, e) = (d.path().join("real"), d.path().join("enlace"));
        fs::write(&a, "hola").unwrap();
        let t = enlazar(&a, &e).unwrap();
        assert_eq!(fs::read_to_string(&e).unwrap(), "hola");
        #[cfg(unix)]
        assert_eq!(t, TipoEnlace::Simbolico);
        let _ = t;
    }

    #[test]
    fn ejecutables() {
        let d = tempfile::tempdir().unwrap();
        assert!(!es_ejecutable(d.path()));
        assert!(!es_ejecutable(&d.path().join("no-existe")));
        #[cfg(unix)]
        {
            let f = d.path().join("f");
            fs::write(&f, "#!/bin/sh\n").unwrap();
            poner_permisos_unix(&f, 0o644).unwrap();
            assert!(!es_ejecutable(&f));
            poner_permisos_unix(&f, 0o755).unwrap();
            assert!(es_ejecutable(&f));
        }
        #[cfg(windows)]
        {
            let (a, b) = (d.path().join("a.exe"), d.path().join("a.txt"));
            fs::write(&a, "").unwrap();
            fs::write(&b, "").unwrap();
            assert!(es_ejecutable(&a) && !es_ejecutable(&b));
        }
    }
}
