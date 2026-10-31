use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::header;
use serde::de::DeserializeOwned;

use crate::error::Error;

/// Strict JSON body: requires `application/json` (415 otherwise), rejects
/// unknown and duplicate-ish extras via the DTO contract (`deny_unknown_fields`),
/// trailing JSON values, and oversized payloads (413, enforced by the body
/// limit layer).
pub struct StrictJson<T>(pub T);

impl<T, S> FromRequest<S> for StrictJson<T>
where
    T: DeserializeOwned + Send + 'static,
    S: Send + Sync,
{
    type Rejection = Error;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        if !json_content_type(request.headers()) {
            return Err(Error::UnsupportedMediaType);
        }
        let bytes = Bytes::from_request(request, state)
            .await
            .map_err(|_| Error::PayloadTooLarge)?;
        serde_json::from_slice::<T>(&bytes)
            .map(StrictJson)
            .map_err(|_| Error::InvalidBody)
    }
}

/// Only `application/json` (with parameters) is accepted. The absence of a
/// Content-Type is a 415 as well, since JSON is the only accept form either way.
fn json_content_type(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}
