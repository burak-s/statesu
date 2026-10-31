use axum::Json;
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

use crate::auth::{AuthResponse, CredentialsRequest, MeResponse, UserRecord};
use crate::error::Error;
use crate::http::{AppState, StrictJson};

pub async fn register(
    State(state): State<AppState>,
    StrictJson(credentials): StrictJson<CredentialsRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), Error> {
    let auth = state.auth_service.register(&credentials).await?;
    Ok((StatusCode::CREATED, Json(auth)))
}

pub async fn login(
    State(state): State<AppState>,
    StrictJson(credentials): StrictJson<CredentialsRequest>,
) -> Result<Json<AuthResponse>, Error> {
    let auth = state.auth_service.login(&credentials).await?;
    Ok(Json(auth))
}

/// Bearer-only API: cookies were never issued by this backend. This endpoint
/// only clears any token cookie a legacy client may still send; bearer tokens
/// stay valid until expiry (no server-side revocation).
pub async fn logout() -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            "token=; Max-Age=0; Path=/; HttpOnly; SameSite=Strict; Secure",
        )],
    )
        .into_response()
}

pub async fn me(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<MeResponse>, Error> {
    let me = state.auth_service.me(&user).await?;
    Ok(Json(me))
}

/// Extracts and authenticates the bearer token from the Authorization header;
/// users are resolved from the database so revoked accounts are rejected.
#[derive(Debug, Clone)]
pub struct AuthedUser(pub UserRecord);

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(Error::MissingToken)?;
        let token = header_value
            .strip_prefix("Bearer ")
            .filter(|token| !token.is_empty())
            .ok_or(Error::MissingToken)?;
        let user = state.auth_service.authenticate(token).await?;
        Ok(AuthedUser(user))
    }
}
