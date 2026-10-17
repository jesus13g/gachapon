//! Capa HTTP: la página estática, la guardia de la API local y las rutas
//! JSON. Aquí solo se traduce: peticiones a llamadas de `servicio` y sus
//! vistas y errores a respuestas.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::{tema, AppState};

mod error;
mod rutas;

pub use error::ApiError;

pub fn router(state: Arc<AppState>) -> Router {
    use rutas::*;
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(app_css))
        .route("/app.js", get(app_js))
        .route("/cliente.js", get(cliente_js))
        .route("/gashapon.css", get(gashapon_css))
        .route("/gashapon.js", get(gashapon_js))
        .route("/tema.css", get(tema_css))
        .route("/api/estado", get(estado_general))
        .route("/api/inicio", get(inicio))
        .route("/api/diario", get(diario))
        .route("/api/rutinas", get(rutinas_listar))
        .route("/api/rutinas/{slug}", get(rutina_ver))
        .route("/api/rutinas/{slug}/lanzar", post(rutina_lanzar))
        .route("/api/trabajos", get(trabajos_listar))
        .route("/api/trabajos/{id}", get(trabajo_ver))
        .route("/api/trabajos/{id}/diff", get(trabajo_diff))
        .route("/api/trabajos/{id}/cerrar", post(trabajo_cerrar))
        .route("/api/trabajos/{id}/cancelar", post(trabajo_cancelar))
        .route("/api/trabajos/{id}/continuar", post(trabajo_continuar))
        .route("/api/trabajos/{id}/reformular", post(trabajo_reformular))
        .route("/api/trabajos/{id}/confirmar", post(trabajo_confirmar))
        .route("/api/colas", get(colas))
        .route("/api/semaforos", get(semaforos))
        .route("/api/sesiones", get(sesiones))
        .route("/api/propuestas", get(propuestas_listar))
        .route("/api/propuestas/{id}/decidir", post(propuesta_decidir))
        .route("/api/acciones", get(acciones_listar))
        .route("/api/acciones/simular", post(accion_simular))
        .route("/api/acciones/ejecutar", post(accion_ejecutar))
        .route("/api/boveda", get(boveda_listar))
        .route("/api/boveda/nota", get(boveda_leer))
        .route("/api/boveda/promover", post(boveda_promover))
        .route("/api/servicios/{nombre}/reiniciar", post(servicio_reiniciar))
        .layer(middleware::from_fn(guardia))
        .with_state(state)
}

/// Defensa de una API local frente a páginas ajenas abiertas en el mismo
/// navegador: solo se atiende `Host: localhost|127.0.0.1` (un ataque de
/// reenlace DNS llega con otro host) y toda escritura debe declarar
/// `application/json`, lo que obliga a una petición previa (preflight) que
/// este servidor nunca autoriza.
async fn guardia(req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            let nombre = match h.rsplit_once(':') {
                Some((n, puerto)) if puerto.bytes().all(|b| b.is_ascii_digit()) => n,
                _ => h,
            };
            nombre.eq_ignore_ascii_case("localhost") || nombre == "127.0.0.1"
        });
    if !host_ok {
        return ApiError::new(StatusCode::FORBIDDEN, "host no permitido").into_response();
    }

    if !matches!(*req.method(), Method::GET | Method::HEAD) {
        let es_json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        if !es_json {
            return ApiError::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "se espera application/json")
                .into_response();
        }
    }
    next.run(req).await
}

// ---------------------------------------------------------------- estático

fn estatico(tipo: &'static str, cuerpo: String) -> Response {
    (
        [(header::CONTENT_TYPE, tipo), (header::CACHE_CONTROL, "no-store")],
        cuerpo,
    )
        .into_response()
}

async fn index() -> Response {
    estatico("text/html; charset=utf-8", include_str!("../../static/index.html").into())
}

async fn app_css() -> Response {
    estatico("text/css; charset=utf-8", include_str!("../../static/app.css").into())
}

async fn app_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../../static/app.js").into())
}

async fn cliente_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../../static/cliente.js").into())
}

async fn gashapon_css() -> Response {
    estatico("text/css; charset=utf-8", include_str!("../../static/gashapon.css").into())
}

async fn gashapon_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../../static/gashapon.js").into())
}

async fn tema_css(State(s): State<Arc<AppState>>) -> Response {
    estatico("text/css; charset=utf-8", tema::css(&s.config.tema_path))
}
