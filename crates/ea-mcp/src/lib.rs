pub mod args;
pub mod credencial;
pub mod tools;

pub use credencial::Credencial;
pub use tools::EaTools;

use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager};
use tokio_util::sync::CancellationToken;

/// Los únicos `Host` que se atienden: una página ajena que reenlace su nombre
/// a 127.0.0.1 (DNS rebinding) llega con el suyo y recibe un 403. Es lo que
/// rmcp trae por defecto, pero escrito aquí para que el «solo local» no
/// dependa de un valor por defecto de la dependencia.
pub const HOSTS_PERMITIDOS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// El servidor de herramientas en `/mcp`, con el filtro de `Host`. `fabrica`
/// crea las herramientas de cada sesión (así una sesión nueva ve los ajustes
/// de ahora) y cancelar `parar` cierra todas las sesiones abiertas. Lo usan
/// `main` y las pruebas.
pub fn router<F>(fabrica: F, parar: CancellationToken) -> axum::Router
where
    F: Fn() -> EaTools + Send + Sync + 'static,
{
    let servicio = StreamableHttpService::new(
        move || Ok(fabrica()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().with_allowed_hosts(HOSTS_PERMITIDOS).with_cancellation_token(parar),
    );
    axum::Router::new().nest_service("/mcp", servicio)
}
