//! `Cuerpo<T>`: el extractor de todos los cuerpos JSON de la API. Cualquier
//! rechazo (JSON mal formado, un campo que sobra o falta, un tipo que no
//! encaja, un cuerpo que no se puede leer) es un 400 `{"error": "…"}`, como
//! los errores de dominio; nunca el texto plano ni el 422 de `axum::Json`.
//! El tipo de contenido ya lo exige la guardia (`application/json`, si no
//! también 400) antes de llegar aquí. Un cuerpo vacío cuenta como `{}`.

use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use serde::de::DeserializeOwned;

use super::ApiError;

pub struct Cuerpo<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Cuerpo<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, estado: &S) -> Result<Self, ApiError> {
        let bytes = Bytes::from_request(req, estado)
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("no se pudo leer el cuerpo: {}", e.body_text())))?;
        let bytes: &[u8] = if bytes.iter().all(u8::is_ascii_whitespace) { b"{}" } else { &bytes };
        serde_json::from_slice(bytes).map(Cuerpo).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, format!("cuerpo inválido: {e}")))
    }
}
