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

use crate::{clave, tema, AppState};

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
        .route("/enlaces.js", get(enlaces_js))
        .route("/gashapon.css", get(gashapon_css))
        .route("/gashapon.js", get(gashapon_js))
        .route("/tour.js", get(tour_js))
        .route("/tema.css", get(tema_css))
        .route("/api/estado", get(estado_general))
        .route("/api/inicio", get(inicio))
        .route("/api/diario", get(diario))
        .route("/api/rutinas", get(rutinas_listar))
        .route("/api/rutinas/validar", get(rutina_ver_validar).post(rutina_validar))
        .route("/api/rutinas/registrar", get(rutina_ver_registrar).post(rutina_registrar))
        .route("/api/rutinas/{slug}", get(rutina_ver))
        .route("/api/rutinas/{slug}/lanzar", post(rutina_lanzar))
        .route("/api/rutinas/{slug}/previa", post(rutina_previa))
        .route("/api/trabajos", get(trabajos_listar).post(trabajo_crear))
        .route("/api/trabajos/nuevo", get(trabajo_formulario))
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
        .route("/api/boveda/arbol", get(boveda_arbol))
        .route("/api/boveda/fichero", get(boveda_fichero))
        .route("/api/boveda/guardar", post(boveda_guardar))
        .route("/api/boveda/crear", post(boveda_crear))
        .route("/api/boveda/renombrar", post(boveda_renombrar))
        .route("/api/boveda/eliminar", post(boveda_eliminar))
        .route("/api/boveda/carpeta", post(boveda_carpeta))
        .route("/api/boveda/mostrar", post(boveda_mostrar))
        .route("/api/boveda/papelera", get(boveda_papelera))
        .route("/api/boveda/papelera/restaurar", post(boveda_restaurar))
        .route("/api/memoria", get(memoria_listar))
        .route("/api/memoria/{proyecto}/{fichero}", get(memoria_leer).post(memoria_guardar))
        .route("/api/memoria/{proyecto}/{fichero}/eliminar", post(memoria_eliminar))
        .route("/api/memoria/{proyecto}/{fichero}/renombrar", post(memoria_renombrar))
        .route("/api/memoria/{proyecto}/{fichero}/mostrar", post(memoria_mostrar))
        .route("/api/servicios/{nombre}/reiniciar", post(servicio_reiniciar))
        .route("/api/ajustes", get(ajustes_ver).post(ajustes_cambiar))
        .route("/api/sesion", get(sesion))
        .layer(middleware::from_fn_with_state(state.clone(), guardia))
        .with_state(state)
}

/// Si la clave que trae la página (en `X-EA-Clave`, opcional) es la de este
/// arranque: la página lo pregunta al cargar para saber si puede escribir.
async fn sesion(State(s): State<Arc<AppState>>, headers: header::HeaderMap) -> Response {
    let escritura = headers.get(clave::CABECERA).is_some_and(|v| s.clave.coincide(v.as_bytes()));
    axum::Json(serde_json::json!({ "escritura": escritura })).into_response()
}

/// Defensa de una API local frente a páginas ajenas abiertas en el mismo
/// navegador: solo se atiende `Host: localhost|127.0.0.1` (un ataque de
/// reenlace DNS llega con otro host) y toda escritura debe declarar
/// `application/json`, lo que obliga a una petición previa (preflight) que
/// este servidor nunca autoriza.
///
/// Frente a otros procesos locales (el agente local tiene la red abierta),
/// todo lo que no sea GET/HEAD debe traer además la clave de sesión del
/// arranque en `X-EA-Clave` (ver `clave`).
async fn guardia(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
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
        let mensaje = match req.headers().get(clave::CABECERA) {
            None => Some(
                "falta la clave de sesión del panel (cabecera X-EA-Clave): ábrelo con ea-web-open para poder cambiar nada",
            ),
            Some(v) if !s.clave.coincide(v.as_bytes()) => Some(
                "la clave de sesión del panel no es válida (¿se reinició ea-web?): cierra la ventana y vuelve a abrirla con ea-web-open",
            ),
            Some(_) => None,
        };
        if let Some(m) = mensaje {
            let mut r = ApiError::new(StatusCode::FORBIDDEN, m).into_response();
            // Para que la página sepa que es la clave y pase a solo lectura.
            r.headers_mut().insert(clave::CABECERA, header::HeaderValue::from_static("requerida"));
            return r;
        }
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

async fn enlaces_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../../static/enlaces.js").into())
}

async fn gashapon_css() -> Response {
    estatico("text/css; charset=utf-8", include_str!("../../static/gashapon.css").into())
}

async fn gashapon_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../../static/gashapon.js").into())
}

async fn tour_js() -> Response {
    estatico("text/javascript; charset=utf-8", include_str!("../../static/tour.js").into())
}

async fn tema_css(State(s): State<Arc<AppState>>) -> Response {
    estatico("text/css; charset=utf-8", tema::css(&s.config.tema_path))
}
