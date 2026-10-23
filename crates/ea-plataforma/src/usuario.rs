//! El usuario que corre el proceso.

/// El directorio personal del usuario: `$HOME` en Unix y `%USERPROFILE%` en
/// Windows (como `std::env::home_dir`), y el del sistema si la variable falta.
/// Es lo que respetan las pruebas para no tocar el home real; `directories` en
/// Windows ignora la variable y devuelve siempre el del usuario real.
pub fn home() -> Option<std::path::PathBuf> {
    #[allow(deprecated)]
    std::env::home_dir().filter(|p| !p.as_os_str().is_empty())
}

/// El uid numérico del usuario en Unix (lo piden `launchctl gui/<uid>` y
/// compañía). `None` en Windows, donde no existe.
pub fn uid() -> Option<u32> {
    #[cfg(unix)]
    {
        // SAFETY: `getuid` no tiene precondiciones ni puede fallar.
        Some(unsafe { libc::getuid() })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(unix)]
    fn hay_uid_en_unix() {
        assert!(super::uid().is_some());
    }
}
