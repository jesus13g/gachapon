//! Espera de la señal de parada del proceso (solo con la feature `tokio`).

use std::future::Future;

/// Futuro que resuelve cuando el sistema pide parar: SIGTERM o Ctrl-C en
/// Unix; Ctrl-C, cierre de consola o apagado en Windows.
///
/// Los manejadores se instalan al llamarla, no al esperar el futuro: un
/// servicio la llama antes de escuchar para que una parada temprana (systemd
/// que reinicia nada más arrancar) no lo mate sin cerrar limpio. Hay que
/// llamarla dentro del runtime de tokio.
pub fn esperar_parada() -> impl Future<Output = ()> + Send + 'static {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let term = signal(SignalKind::terminate()).ok();
        let int = signal(SignalKind::interrupt()).ok();
        async move {
            // Una señal que no se pudo instalar no resuelve nunca.
            let recibir = |s: Option<tokio::signal::unix::Signal>| async move {
                match s {
                    Some(mut s) => {
                        s.recv().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                _ = recibir(term) => {}
                _ = recibir(int) => {}
            }
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_c, ctrl_close, ctrl_shutdown};
        let (c, cierre, apagado) = (ctrl_c().ok(), ctrl_close().ok(), ctrl_shutdown().ok());
        // Una señal que no se pudo instalar no resuelve nunca.
        macro_rules! recibir {
            ($s:expr) => {
                async move {
                    match $s {
                        Some(mut s) => {
                            s.recv().await;
                        }
                        None => std::future::pending::<()>().await,
                    }
                }
            };
        }
        async move {
            tokio::select! {
                _ = recibir!(c) => {}
                _ = recibir!(cierre) => {}
                _ = recibir!(apagado) => {}
            }
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        async {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}
