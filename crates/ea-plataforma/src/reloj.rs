//! Reloj monotónico del sistema.

/// Segundos del reloj monotónico del sistema. En Linux es `CLOCK_MONOTONIC`,
/// el mismo que usa systemd en sus marcas monotónicas (`ActiveEnterTimestampMonotonic`
/// y compañía), así que sirve para convertirlas en «hace cuánto». En macOS
/// también `CLOCK_MONOTONIC`; en Windows, `GetTickCount64` (milisegundos
/// desde el arranque). `None` si el sistema no lo da.
pub fn segundos_desde_arranque() -> Option<f64> {
    #[cfg(unix)]
    {
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: `ts` es una estructura válida y escribible durante la llamada.
        let r = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        (r == 0).then(|| ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9)
    }
    #[cfg(windows)]
    {
        // SAFETY: función sin argumentos ni precondiciones.
        let ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        Some(ms as f64 / 1000.0)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn el_reloj_avanza() {
        let a = super::segundos_desde_arranque().expect("hay reloj");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let b = super::segundos_desde_arranque().unwrap();
        assert!(a > 0.0 && b > a);
    }
}
