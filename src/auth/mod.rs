pub mod repo;
pub mod service;

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: uuid::Uuid,
    pub email_ciphertext: Vec<u8>,
    pub email_hash: Vec<u8>,
    pub password_hash: String,
}

/// Wire response for register/login: token fields plus account identity.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthResponse {
    pub id: uuid::Uuid,
    pub email: String,
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MeResponse {
    pub id: uuid::Uuid,
    pub email: String,
}

/// Strict JSON request body for both register and login.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialsRequest {
    pub email: String,
    pub password: String,
}
