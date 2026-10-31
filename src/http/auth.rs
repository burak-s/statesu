use axum::Json;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::auth::pat_repo::PatRecord;
use crate::auth::service::LoginOutcome;
use crate::auth::{AcceptedResponse, CredentialsRequest, MeResponse, UserRecord, VerifyRequest};
use crate::crypto::token;
use crate::error::Error;
use crate::http::{AppState, StrictJson};

/// Cookie and header names for the browser session. The `__Host-` prefix
/// forces Secure, Path=/ and no Domain attribute in every modern browser.
pub const SESSION_COOKIE: &str = "__Host-statesu_session";
pub const CSRF_COOKIE: &str = "__Host-statesu_csrf";
pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");

pub async fn register(
    State(state): State<AppState>,
    StrictJson(credentials): StrictJson<CredentialsRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), Error> {
    let accepted = state.auth_service.register(&credentials).await?;
    Ok((StatusCode::ACCEPTED, Json(accepted)))
}

pub async fn login(
    State(state): State<AppState>,
    StrictJson(credentials): StrictJson<CredentialsRequest>,
) -> Result<Response, Error> {
    let outcome = state.auth_service.login(&credentials).await?;
    let cookies = session_cookies(&outcome);
    let mut response = Json(outcome.me).into_response();
    for (_, value) in cookies {
        if let Ok(header_value) = value.parse::<HeaderValue>() {
            response
                .headers_mut()
                .append(header::SET_COOKIE, header_value);
        }
    }
    Ok(response)
}

/// Revokes the presented session and clears both cookies; cookie-less calls
/// are authentication errors, not polite 204s.
pub async fn logout(
    State(state): State<AppState>,
    session: AuthedUser,
) -> Result<impl IntoResponse, Error> {
    let token = session.session_token.ok_or(Error::MissingToken)?;
    state.auth_service.logout(&token).await?;
    // Headers are appended explicitly: the tuple machinery deduplicates
    // same-named headers, and there are two Set-Cookie values.
    let mut response = StatusCode::NO_CONTENT.into_response();
    for (_, value) in clear_session_cookies() {
        if let Ok(header_value) = value.parse::<HeaderValue>() {
            response
                .headers_mut()
                .append(header::SET_COOKIE, header_value);
        }
    }
    Ok(response)
}

/// "Sign out everywhere": revokes every session of the account.
pub async fn logout_all(
    State(state): State<AppState>,
    session: AuthedUser,
) -> Result<StatusCode, Error> {
    state.auth_service.logout_all(&session.user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(
    State(state): State<AppState>,
    session: AuthedUser,
) -> Result<Json<MeResponse>, Error> {
    let me = state.auth_service.me(&session.user).await?;
    Ok(Json(me))
}

pub async fn verify(
    State(state): State<AppState>,
    StrictJson(request): StrictJson<VerifyRequest>,
) -> Result<Json<serde_json::Value>, Error> {
    state.auth_service.verify_email(&request).await?;
    Ok(Json(serde_json::json!({ "status": "verified" })))
}

/// Generic `POST /auth/resend`: same answer whatever the address.
pub async fn resend(
    State(state): State<AppState>,
    StrictJson(request): StrictJson<ResendRequest>,
) -> Result<(StatusCode, Json<AcceptedResponse>), Error> {
    let accepted = state
        .auth_service
        .resend_verification(&request.email)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(accepted)))
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResendRequest {
    pub email: String,
}

// Personal access tokens ------------------------------------------------

pub async fn list_pats(
    State(state): State<AppState>,
    session: AuthedUser,
) -> Result<Json<Vec<PatRecord>>, Error> {
    Ok(Json(state.auth_service.list_pats(&session.user.id).await?))
}

pub async fn create_pat(
    State(state): State<AppState>,
    session: AuthedUser,
    StrictJson(request): StrictJson<CreatePatRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), Error> {
    let scopes = if request.write {
        crate::auth::service::PatScopes::read_write()
    } else {
        crate::auth::service::PatScopes::read_only()
    };
    let days = request
        .days
        .unwrap_or(crate::auth::service::PAT_DEFAULT_DAYS);
    let outcome = state
        .auth_service
        .create_pat(&session.user.id, &request.name, scopes, days)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "id": outcome.record.id,
            "name": outcome.record.name,
            "scopes": outcome.record.scopes,
            "created_at": outcome.record.created_at,
            "expires_at": outcome.record.expires_at,
            "secret": outcome.secret,
        })),
    ))
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePatRequest {
    pub name: String,
    pub days: Option<i64>,
    #[serde(default)]
    pub write: bool,
}

pub async fn delete_pat(
    State(state): State<AppState>,
    session: AuthedUser,
    Path(pat_id): Path<String>,
) -> Result<StatusCode, Error> {
    let pat_id = pat_id
        .parse::<uuid::Uuid>()
        .map_err(|_| Error::InvalidInput)?;
    state
        .auth_service
        .delete_pat(&pat_id, &session.user.id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// Authentication plumbing -----------------------------------------------

/// A fully authenticated principal: user, write capability and — for
/// sessions — the raw cookie token so logout can revoke the row.
#[derive(Debug, Clone)]
pub struct AuthedUser {
    pub user: UserRecord,
    /// Sessions always can write; PATs additionally need `content:write`.
    pub can_write: bool,
    /// `None` for PAT-authenticated requests.
    pub session_token: Option<String>,
}

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match parts.headers.get(header::AUTHORIZATION) {
            Some(bearer) => authenticate_bearer(parts, state, bearer).await,
            None => authenticate_session(parts, state).await,
        }
    }
}

/// Write-guarding flavor of [`AuthedUser`], for create/update/delete routes.
/// Read-only PATs and anything else that cannot write are rejected here so
/// handlers themselves never repeat the check.
pub struct AuthedWriter(pub UserRecord);

impl FromRequestParts<AppState> for AuthedWriter {
    type Rejection = Error;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session = AuthedUser::from_request_parts(parts, state).await?;
        if !session.can_write {
            return Err(Error::Forbidden);
        }
        Ok(AuthedWriter(session.user))
    }
}

/// `Authorization: Bearer` → personal access token. PATs serve the content
/// API only; `/auth/*` exclusively belongs to browser sessions.
async fn authenticate_bearer(
    parts: &Parts,
    state: &AppState,
    header_value: &axum::http::HeaderValue,
) -> Result<AuthedUser, Error> {
    if !parts.uri.path().starts_with("/blogs") {
        return Err(Error::InvalidToken);
    }
    let token = header_value
        .to_str()
        .map_err(|_| Error::InvalidToken)?
        .strip_prefix("Bearer ")
        .filter(|token| !token.is_empty() && token::well_formed(token, token::PAT_PREFIX))
        .ok_or(Error::InvalidToken)?;
    let (user, scopes) = state.auth_service.authenticate_pat(token).await?;
    Ok(AuthedUser {
        user,
        can_write: scopes.can_write(),
        session_token: None,
    })
}

/// Server-side session cookie authentication. Unsafe methods additionally
/// require the CSRF token bound to the session and reject cross-site origins.
async fn authenticate_session(parts: &Parts, state: &AppState) -> Result<AuthedUser, Error> {
    let cookies = parts
        .headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or(Error::MissingToken)?;
    let session_token = cookie_value(cookies, SESSION_COOKIE)
        .filter(|value| token::well_formed(value, token::SESSION_PREFIX))
        .ok_or(Error::MissingToken)?;
    let (user, csrf_hash) = state
        .auth_service
        .authenticate_session(session_token)
        .await?;
    if is_unsafe(&parts.method) {
        require_csrf(parts, state)?;
        let supplied = csrf_header_value(parts);
        // The supplied value must hash to the CSRF bound in the session row.
        if token::hash(supplied).as_slice() != csrf_hash.as_slice() {
            return Err(Error::Forbidden);
        }
    }
    Ok(AuthedUser {
        user,
        can_write: true,
        session_token: Some(session_token.to_owned()),
    })
}

fn is_unsafe(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn csrf_header_value(parts: &Parts) -> &str {
    parts
        .headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

fn require_csrf(parts: &Parts, state: &AppState) -> Result<(), Error> {
    // Same-site pin first: a cookie-bearing cross-site write fails even if an
    // attacker also holds the CSRF token.
    let foreign_origin = parts
        .headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| !state.config.public_url.matches_origin(origin));
    if foreign_origin {
        return Err(Error::Forbidden);
    }
    if csrf_header_value(parts).is_empty() {
        return Err(Error::Forbidden);
    }
    Ok(())
}

/// Reads a single cookie value from the `Cookie` header. Token values are
/// unreserved hex, so no %-decoding is needed.
fn cookie_value<'a>(cookies: &'a str, name: &str) -> Option<&'a str> {
    cookies.split(';').map(str::trim).find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then_some(value.trim())
    })
}

/// Set-Cookie pair for a fresh session: HttpOnly session plus the readable
/// CSRF cookie the dashboard echoes back as `X-CSRF-Token`.
fn session_cookies(outcome: &LoginOutcome) -> [(HeaderName, String); 2] {
    [
        (
            header::SET_COOKIE,
            format!(
                "{SESSION_COOKIE}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Lax; Secure",
                outcome.session_token, outcome.max_age_secs
            ),
        ),
        (
            header::SET_COOKIE,
            format!(
                "{CSRF_COOKIE}={}; Max-Age={}; Path=/; SameSite=Lax; Secure",
                outcome.csrf_token, outcome.max_age_secs
            ),
        ),
    ]
}

/// Clears both cookies; the server-side row was already dropped.
fn clear_session_cookies() -> [(HeaderName, &'static str); 2] {
    [
        (
            header::SET_COOKIE,
            "__Host-statesu_session=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax; Secure",
        ),
        (
            header::SET_COOKIE,
            "__Host-statesu_csrf=; Max-Age=0; Path=/; SameSite=Lax; Secure",
        ),
    ]
}
