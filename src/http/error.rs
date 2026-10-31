use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::error::Error;

/// The only IntoResponse mapping for domain errors: every API failure becomes
/// `{"error": "<message>"}` with the status assigned here and nowhere else.
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let message = self.message();
        match self {
            Error::RateLimited { retry_after_secs } => {
                let mut response = error_response(status, message);
                if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                    response.headers_mut().insert(header::RETRY_AFTER, value);
                }
                response
            }
            Error::Internal(internal) => {
                // Internal details go to the log; clients receive one stable message.
                tracing::error!(error = %internal);
                error_response(status, message)
            }
            _ => error_response(status, message),
        }
    }
}

fn error_response(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        [("x-content-type-options", "nosniff")],
        Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}
