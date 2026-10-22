//! Espera de la señal de parada del proceso (solo con la feature `tokio`).

/// Resuelve cuando el sistema pide parar: SIGTERM o Ctrl-C en Unix; Ctrl-C,
/// cierre de consola o apagado en Windows.
pub async fn esperar_parada() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
        let (cierre, apagado) = (ctrl_close(), ctrl_shutdown());
        match (cierre, apagado) {
            (Ok(mut c), Ok(mut a)) => {
                tokio::select! {
                    _ = c.recv() => {}
                    _ = a.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            _ => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
