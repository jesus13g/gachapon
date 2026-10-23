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

/// Desfase en segundos de la hora local respecto de UTC en el instante
/// `unix` (segundos desde 1970), p. ej. 7200 en España en verano. Unix usa
/// `localtime_r`; Windows, `GetTimeZoneInformation` (el desfase vigente, no
/// el de `unix` si cambió el horario de verano entre medias).
pub fn desfase_local(unix: i64) -> Option<i32> {
    #[cfg(unix)]
    {
        let t = unix as libc::time_t;
        // SAFETY: `tm` es una estructura válida y escribible; `localtime_r`
        // es reentrante y solo lee `t`.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let r = unsafe { libc::localtime_r(&t, &mut tm) };
        (!r.is_null()).then_some(tm.tm_gmtoff as i32)
    }
    #[cfg(windows)]
    {
        let _ = unix;
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        // SAFETY: estructura válida y escribible durante la llamada.
        let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        let estado = unsafe { GetTimeZoneInformation(&mut tz) };
        // TIME_ZONE_ID_INVALID = 0xFFFFFFFF; 2 = horario de verano.
        if estado == u32::MAX {
            return None;
        }
        let extra = if estado == 2 { tz.DaylightBias } else { tz.StandardBias };
        Some(-(tz.Bias + extra) * 60)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = unix;
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn el_desfase_local_es_razonable() {
        let d = super::desfase_local(1_790_000_000).expect("hay zona horaria");
        assert!(d.abs() <= 14 * 3600 && d % 60 == 0, "{d}");
    }

    #[test]
    fn el_reloj_avanza() {
        let a = super::segundos_desde_arranque().expect("hay reloj");
        std::thread::sleep(std::time::Duration::from_millis(20));
        let b = super::segundos_desde_arranque().unwrap();
        assert!(a > 0.0 && b > a);
    }
}
