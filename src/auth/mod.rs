pub mod cli_repo;
pub mod pat_repo;
pub mod repo;
pub mod service;
pub mod session_repo;
pub mod turnstile;
pub mod verification_repo;

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: uuid::Uuid,
    pub email_ciphertext: Vec<u8>,
    pub email_hash: Vec<u8>,
    pub password_hash: String,
    /// `None` until the address has been confirmed by email.
    pub email_verified_at: Option<i64>,
}

impl UserRecord {
    pub fn is_verified(&self) -> bool {
        self.email_verified_at.is_some()
    }
}

/// Identity payload for `/auth/me` and the login response body. No token
/// fields: sessions live in HttpOnly cookies, API tokens in labeled PATs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MeResponse {
    pub id: uuid::Uuid,
    pub email: String,
}

/// Strict JSON request body for register and login. `turnstile` is only read
// when the server has a Turnstile secret configured; unknown fields are still
// rejected so the body cannot smuggle extra state.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialsRequest {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub turnstile: Option<String>,
}

/// Generic 202 body for register and resend: deliberately identical for
/// existing and unknown addresses so the endpoint cannot confirm accounts.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcceptedResponse {
    pub status: &'static str,
}

impl AcceptedResponse {
    pub const VERIFICATION_REQUIRED: AcceptedResponse = AcceptedResponse {
        status: "verification_required",
    };
}

/// Strict JSON body for `POST /auth/verify`.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyRequest {
    pub token: String,
}
