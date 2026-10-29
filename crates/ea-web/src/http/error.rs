//! Errores de la API: `{ "error": mensaje }` con el código que corresponde
//! a cada error de dominio.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::servicio::ErrorWeb;

pub struct ApiError(StatusCode, String);

impl ApiError {
    pub fn new(status: StatusCode, msg: impl Into<String>) -> Self {
        Self(status, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<ErrorWeb> for ApiError {
    fn from(e: ErrorWeb) -> Self {
        let (status, msg) = match e {
            ErrorWeb::NoEncontrado(m) => (StatusCode::NOT_FOUND, m),
            ErrorWeb::Invalido(m) => (StatusCode::BAD_REQUEST, m),
            ErrorWeb::Conflicto(m) => (StatusCode::CONFLICT, m),
            ErrorWeb::Prohibido(m) => (StatusCode::FORBIDDEN, m),
            ErrorWeb::NoDisponible(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            ErrorWeb::Tiempo(m) => (StatusCode::GATEWAY_TIMEOUT, m),
            ErrorWeb::Interno(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        Self(status, msg)
    }
}
