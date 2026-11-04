//! Ficheros: permisos privados, enlaces, renombrado sin pisar, ejecutables,
//! escritura atómica y huella del contenido.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Crea el directorio (y los padres) y, en Unix, lo deja en 0700. Los
/// directorios nuevos nacen ya con 0700 (sin ventana con la máscara por
/// defecto); el `set_permissions` final cubre la hoja si ya existía.
pub fn crear_dir_privado(ruta: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().recursive(true).mode(0o700).create(ruta)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(ruta)?;
    poner_privado(ruta)
}

/// Crea UN directorio nuevo (falla con `AlreadyExists` si ya está, sin crear
/// padres) que nace con 0700 en Unix.
pub fn crear_dir_nuevo_privado(ruta: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(ruta)
    }
    #[cfg(not(unix))]
    fs::create_dir(ruta)
}

/// `OpenOptions` que, al crear un fichero, lo hace con 0600 en Unix. En
/// Windows son las opciones normales (el perfil del usuario ya protege).
pub fn opciones_privadas() -> OpenOptions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut o = OpenOptions::new();
        o.mode(0o600);
        o
    }
    #[cfg(not(unix))]
    {
        OpenOptions::new()
    }
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
            Some(libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP) => {
                ErrorRenombrar::NoSoportado(format!("el sistema de ficheros no admite mover sin riesgo de pisar algo (RENAME_NOREPLACE/RENAME_EXCL): {e}"))
            }
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
            exts.contains(&e)
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

/// Con qué permisos queda el fichero que escribe [`escribir_atomico`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permisos {
    /// Los de un fichero nuevo cualquiera (en Unix, 0666 menos la máscara).
    Heredados,
    /// Solo para el usuario: en Unix nace con 0600, sin ventana.
    Privados,
    /// Los del fichero que se sustituye, que tiene que existir: los bits de
    /// modo en Unix, el atributo de solo lectura en Windows. El temporal nace
    /// con 0600 y los toma antes de escribir nada.
    ComoOriginal,
}

static CONTADOR_TEMPORAL: AtomicU64 = AtomicU64::new(0);
/// Nombres de temporal que se prueban si ya hay algo con ese nombre (el
/// huérfano de un proceso anterior con el mismo pid, o un enlace plantado):
/// lo que hay no se abre ni se sigue, se pasa al siguiente.
const INTENTOS_TEMPORAL: u32 = 16;

/// Escribe `bytes` en `destino` de forma atómica: un lector ve el fichero
/// anterior o el nuevo entero, nunca una mitad. Temporal oculto
/// `.{nombre}.{pid}.{n}.tmp` en el mismo directorio, creado con
/// `create_new` (no sigue un enlace plantado con ese nombre), `fsync`,
/// renombrado sobre `destino` y `fsync` del directorio. Si algo falla, el
/// temporal se borra y `destino` queda como estaba. No crea el directorio
/// padre.
pub fn escribir_atomico(destino: &Path, bytes: &[u8], permisos: Permisos) -> io::Result<()> {
    let dir = destino.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "ruta sin directorio"))?;
    let original = match permisos {
        Permisos::ComoOriginal => Some(fs::metadata(destino)?),
        Permisos::Heredados | Permisos::Privados => None,
    };
    let nombre = destino.file_name().and_then(|n| n.to_str()).unwrap_or("fichero");
    let mut intento = 0;
    let (temporal, mut f) = loop {
        let n = CONTADOR_TEMPORAL.fetch_add(1, Ordering::Relaxed);
        let temporal = dir.join(format!(".{nombre}.{}.{n}.tmp", std::process::id()));
        let mut o = if permisos == Permisos::Heredados { OpenOptions::new() } else { opciones_privadas() };
        match o.write(true).create_new(true).open(&temporal) {
            Ok(f) => break (temporal, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && intento + 1 < INTENTOS_TEMPORAL => intento += 1,
            Err(e) => return Err(e),
        }
    };
    let resultado = (|| -> io::Result<()> {
        if let Some(meta) = &original {
            copiar_permisos(&f, meta)?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temporal, destino)
    })();
    if let Err(e) = resultado {
        let _ = fs::remove_file(&temporal);
        return Err(e);
    }
    sincronizar_dir(dir);
    Ok(())
}

/// Pone en `f` los permisos de `meta`: los bits de modo en Unix, el atributo
/// de solo lectura en Windows.
fn copiar_permisos(f: &File, meta: &Metadata) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(meta.permissions().mode() & 0o7777))
    }
    #[cfg(not(unix))]
    {
        f.set_permissions(meta.permissions())
    }
}

/// FNV-1a de 64 bits: estable entre procesos, sistemas y versiones de Rust
/// (lo que no garantiza `std::hash`). Sirve para versiones de contenido y
/// nombres cortos derivados de una ruta, no para nada criptográfico.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// [`fnv64`] en 16 cifras hexadecimales.
pub fn huella(bytes: &[u8]) -> String {
    format!("{:016x}", fnv64(bytes))
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

    #[cfg(unix)]
    #[test]
    fn un_arbol_nuevo_nace_todo_0700() {
        let d = tempfile::tempdir().unwrap();
        let hoja = d.path().join("a/b/c");
        crear_dir_privado(&hoja).unwrap();
        for r in ["a", "a/b", "a/b/c"] {
            assert_eq!(permisos_unix(&fs::metadata(d.path().join(r)).unwrap()), Some(0o700), "{r}");
        }
        // Una hoja que ya existía con otro modo también queda 0700.
        let otra = d.path().join("z");
        fs::create_dir(&otra).unwrap();
        poner_permisos_unix(&otra, 0o755).unwrap();
        crear_dir_privado(&otra).unwrap();
        assert!(es_privado(&otra).unwrap());
        let n = d.path().join("n");
        crear_dir_nuevo_privado(&n).unwrap();
        assert!(es_privado(&n).unwrap());
        assert_eq!(crear_dir_nuevo_privado(&n).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
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

    #[test]
    fn escribir_atomico_sustituye_sin_dejar_temporales_ni_crear_el_padre() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("f.json");
        escribir_atomico(&f, b"uno", Permisos::Heredados).unwrap();
        escribir_atomico(&f, b"dos", Permisos::Heredados).unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"dos");
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 1, "sin temporales");
        let e = escribir_atomico(&d.path().join("falta/f"), b"x", Permisos::Heredados).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert!(!d.path().join("falta").exists());
    }

    #[cfg(unix)]
    #[test]
    fn escribir_atomico_con_cada_permiso() {
        let d = tempfile::tempdir().unwrap();
        let modo = |p: &Path| permisos_unix(&fs::metadata(p).unwrap()).unwrap();
        // Heredados: los de cualquier fichero nuevo del proceso.
        let normal = d.path().join("normal");
        fs::write(&normal, "x").unwrap();
        let h = d.path().join("heredados");
        escribir_atomico(&h, b"h", Permisos::Heredados).unwrap();
        assert_eq!(modo(&h), modo(&normal));
        // Privados: 0600 aunque el fichero sustituido fuera 0644.
        let p = d.path().join("privados");
        fs::write(&p, "viejo").unwrap();
        poner_permisos_unix(&p, 0o644).unwrap();
        escribir_atomico(&p, b"p", Permisos::Privados).unwrap();
        assert_eq!(modo(&p), 0o600);
        // ComoOriginal: los del fichero sustituido, también los de ejecución.
        for m in [0o640, 0o755] {
            let c = d.path().join(format!("como-{m:o}"));
            fs::write(&c, "viejo").unwrap();
            poner_permisos_unix(&c, m).unwrap();
            escribir_atomico(&c, b"c", Permisos::ComoOriginal).unwrap();
            assert_eq!((modo(&c), fs::read(&c).unwrap()), (m, b"c".to_vec()));
        }
        // ComoOriginal exige que el original exista.
        let e = escribir_atomico(&d.path().join("no-existe"), b"x", Permisos::ComoOriginal).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 5, "sin temporales");
    }

    #[cfg(unix)]
    #[test]
    fn un_enlace_plantado_con_el_nombre_del_temporal_no_se_sigue() {
        let d = tempfile::tempdir().unwrap();
        let victima = d.path().join("victima");
        fs::write(&victima, "intacta").unwrap();
        let dir = d.path().join("dir");
        fs::create_dir(&dir).unwrap();
        let destino = dir.join("f");
        fs::write(&destino, "viejo").unwrap();
        let plantar = |cuantos: u64| {
            let desde = CONTADOR_TEMPORAL.load(Ordering::Relaxed);
            for n in desde..desde + cuantos {
                std::os::unix::fs::symlink(&victima, dir.join(format!(".f.{}.{n}.tmp", std::process::id()))).unwrap();
            }
        };
        // Los próximos nombres de temporal ya son enlaces: se saltan.
        plantar(8);
        escribir_atomico(&destino, b"nuevo", Permisos::Heredados).unwrap();
        assert_eq!(fs::read_to_string(&destino).unwrap(), "nuevo");
        assert_eq!(fs::read_to_string(&victima).unwrap(), "intacta");
        // Si chocan todos los intentos, falla sin tocar nada.
        plantar(1000);
        let e = escribir_atomico(&destino, b"otro", Permisos::Privados).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&destino).unwrap(), "nuevo");
        assert_eq!(fs::read_to_string(&victima).unwrap(), "intacta");
    }

    /// Prueba dorada: las copias de FNV-1a que había repartidas por el
    /// workspace antes de unificarlas aquí, copiadas tal cual, dan lo mismo
    /// que [`fnv64`] y [`huella`]. La de `ea-ajustes` (vacío si no hay
    /// fichero) tiene la suya en ese crate.
    mod huella_dorada {
        use super::super::{fnv64, huella};

        // ea-bus/src/fs_atomico.rs (`fnv`, para `nuevo_id` y el `.lock` del semáforo).
        fn bus(texto: &str) -> u64 {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in texto.bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        }

        // ea-boveda/src/boveda.rs (`huella`).
        fn boveda(texto: &str) -> String {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in texto.bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x100000001b3);
            }
            format!("{h:016x}")
        }

        // ea-web/src/memoria.rs (`version`).
        fn memoria(contenido: &[u8]) -> String {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in contenido {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x100000001b3);
            }
            format!("{h:016x}")
        }

        // ea-acciones/src/herramientas/mod.rs (`huella`).
        fn herramientas(texto: &str) -> String {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in texto.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            format!("{h:016x}")
        }

        // ea-biblioteca/src/sesion.rs (`huella`).
        fn sesion(texto: &str) -> String {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in texto.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            format!("{h:016x}")
        }

        // ea-biblioteca/src/uso/cache.rs (dentro de `cabeza_de`).
        fn cabeza(buf: &[u8]) -> u64 {
            let mut h: u64 = 0xcbf29ce484222325;
            for b in buf {
                h ^= u64::from(*b);
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        }

        const TEXTOS: [&str; 6] = ["", "a", "foobar", "/home/u/proy\0-1234-7", "ñandú, cigüeña — 漢字 🦀", "línea\r\n\tcon\u{0}nul"];

        #[test]
        fn las_copias_antiguas_dan_lo_mismo() {
            for t in TEXTOS {
                let b = t.as_bytes();
                assert_eq!(bus(t), fnv64(b), "{t:?}");
                assert_eq!(cabeza(b), fnv64(b), "{t:?}");
                for vieja in [boveda(t), memoria(b), herramientas(t), sesion(t)] {
                    assert_eq!(vieja, huella(b), "{t:?}");
                }
            }
            // Bytes que no son UTF-8 (cabeceras de fichero, contenido de una memoria).
            let raros = [0xff, 0x00, 0xfe, 0x80, 0x7f];
            assert_eq!((cabeza(&raros), memoria(&raros)), (fnv64(&raros), huella(&raros)));
        }

        #[test]
        fn valores_de_referencia_de_fnv1a_64() {
            assert_eq!(fnv64(b""), 0xcbf29ce484222325);
            assert_eq!(fnv64(b"a"), 0xaf63dc4c8601ec8c);
            assert_eq!(fnv64(b"foobar"), 0x85944171f73967e8);
            assert_eq!(huella(b""), "cbf29ce484222325");
            assert_eq!(huella(b"a").len(), 16);
        }
    }
}
