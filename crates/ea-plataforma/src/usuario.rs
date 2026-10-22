//! El usuario que corre el proceso.

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
