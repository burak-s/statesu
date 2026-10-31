use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::error::Error;
use crate::http::rate_limit::RateLimiter;
use crate::http::static_files::fallback as docs_fallback;

pub mod auth;
pub mod client_ip;
pub mod error;
pub mod json_body;
pub mod rate_limit;
pub mod state;
pub mod static_files;

pub use json_body::StrictJson;

/// Shared application services injected through the router state only.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub auth_service: crate::auth::service::AuthService,
    pub states: crate::state::service::StateService,
    pub assets: Arc<dyn static_files::StaticAssets>,
    pub limiters: Limiters,
    pub concurrency_gate: Arc<Semaphore>,
    /// Content-Security-Policy for the docs responses.
    pub csp: Arc<str>,
}

#[derive(Clone)]
pub struct Limiters {
    /// Everything except register/login.
    pub general: Arc<RateLimiter>,
    /// Register/login attempts only; much tighter.
    pub auth: Arc<RateLimiter>,
}

/// Request timeouts: reads get 10 seconds, writes 5 seconds, measured around
/// the whole handler including request body consumption.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// JSON body limits: auth endpoints accept 16 KiB, state creation 32 KiB.
const AUTH_BODY_LIMIT: usize = 16 * 1024;
const STATE_BODY_LIMIT: usize = 32 * 1024;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Assembles the production router: API routes guarded by CORS + rate limits,
/// plus an embedded docs fallback returning 404/405.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/state", get(state::list).post(state::create))
        .route("/state/latest", get(state::latest))
        .route("/state/{state_id}", delete(state::delete))
        // Onion order (outermost last): concurrency, guard, then request core.
        .layer(middleware::from_fn_with_state(state.clone(), request_core))
        .layer(middleware::from_fn_with_state(state.clone(), request_guard))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            concurrency_gate,
        ))
        .fallback(docs_fallback)
        .with_state(state)
}

/// Concurrency gate: requests beyond the configured maximum get a controlled
/// 503 instead of queueing unboundedly.
pub async fn concurrency_gate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    match state.concurrency_gate.clone().try_acquire_owned() {
        Ok(permit) => {
            let response = next.run(request).await;
            drop(permit);
            response
        }
        Err(_) => Error::ServiceUnavailable.into_response(),
    }
}

/// CORS + rate limiting + cache-control: covers API responses and preflights.
pub async fn request_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(preflight) = preflight_response(&state, &request) {
        return preflight;
    }

    let class = RouteClass::for_request(&request);
    let client = request_client_ip(&state, &request);
    let origin = request.headers().get(header::ORIGIN).cloned();
    let has_authorization = request.headers().contains_key(header::AUTHORIZATION);

    let limiter = match class {
        RouteClass::SensitiveAuth => &state.limiters.auth,
        _ => &state.limiters.general,
    };
    let mut response = match limiter.take_slot(client) {
        Ok(()) => next.run(request).await,
        Err(retry_after_secs) => Error::RateLimited { retry_after_secs }.into_response(),
    };
    apply_cors(&state, class, origin.as_ref(), &mut response, false);
    add_response_headers(&mut response, class, has_authorization);
    response
}

/// Timeouts and body-size guards applied around the handler itself.
pub async fn request_core(request: Request, next: Next) -> Response {
    let is_write = request.method() != Method::GET && request.method() != Method::HEAD;
    let timeout = if is_write {
        WRITE_TIMEOUT
    } else {
        READ_TIMEOUT
    };
    let request = match body_limit_for(request.method(), request.uri().path()) {
        Some(limit) => limit_body(request, limit),
        None => request,
    };
    match tokio::time::timeout(timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => Error::RequestTimeout.into_response(),
    }
}

fn body_limit_for(method: &Method, path: &str) -> Option<usize> {
    if method == Method::POST {
        Some(if path == "/state" {
            STATE_BODY_LIMIT
        } else {
            AUTH_BODY_LIMIT
        })
    } else {
        None
    }
}

fn limit_body(request: Request, limit: usize) -> Request {
    let (parts, body) = request.into_parts();
    Request::from_parts(
        parts,
        axum::body::Body::new(http_body_util::Limited::new(body, limit)),
    )
}

/// Route behavior classes decided from method + path before authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RouteClass {
    /// GET state reads: any origin may call them cross-origin, no credentials.
    PublicReads,
    /// register/login: CORS allowlist plus a tight quota.
    SensitiveAuth,
    /// Other auth routes and writes: CORS allowlist, general quota.
    Restricted,
}

impl RouteClass {
    fn for_request(request: &Request) -> RouteClass {
        if request.method() == Method::OPTIONS {
            return class_for_access_control_method(
                request
                    .headers()
                    .get(header::ACCESS_CONTROL_REQUEST_METHOD)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| Method::from_bytes(value.as_bytes()).ok())
                    .as_ref(),
                request.uri().path(),
            );
        }
        class_for_method(request.method(), request.uri().path())
    }
}

fn class_for_method(method: &Method, path: &str) -> RouteClass {
    let is_read = method == Method::GET || method == Method::HEAD;
    if is_read && matches!(path, "/state" | "/state/latest") {
        RouteClass::PublicReads
    } else if matches!(path, "/auth/register" | "/auth/login") {
        RouteClass::SensitiveAuth
    } else {
        RouteClass::Restricted
    }
}

fn class_for_access_control_method(method: Option<&Method>, path: &str) -> RouteClass {
    class_for_method(method.unwrap_or(&Method::GET), path)
}

/// Preflight short-circuit: OPTIONS with Origin and an
/// Access-Control-Request-Method is answered here.
fn preflight_response(state: &AppState, request: &Request) -> Option<Response> {
    if request.method() != Method::OPTIONS {
        return None;
    }
    let origin = request.headers().get(header::ORIGIN)?;
    if !request
        .headers()
        .contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
    {
        return None;
    }
    let class = RouteClass::for_request(request);
    let mut response = StatusCode::NO_CONTENT.into_response();
    apply_cors(state, class, Some(origin), &mut response, true);
    Some(response)
}

/// Applies the CORS policy. Public reads are open to any origin without
/// credentials; auth/write routes require an exact allowlist match. Credentialed
/// CORS is deliberately never enabled.
fn apply_cors(
    state: &AppState,
    class: RouteClass,
    origin: Option<&HeaderValue>,
    response: &mut Response,
    preflight: bool,
) {
    let Some(origin) = origin else {
        return;
    };
    let headers = response.headers_mut();
    let allowed = match class {
        RouteClass::PublicReads => {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            true
        }
        _ => {
            let Ok(origin_str) = origin.to_str() else {
                return;
            };
            if !state
                .config
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin_str)
            {
                return;
            }
            headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
            headers.append(header::VARY, HeaderValue::from_static("Origin"));
            true
        }
    };
    if !allowed || !preflight {
        return;
    }
    // Preflight response headers are only useful when the origin was allowed.
    let methods = match class {
        RouteClass::PublicReads => "GET, HEAD",
        _ => "GET, HEAD, POST, DELETE",
    };
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_str(methods).unwrap_or_else(|_| HeaderValue::from_static("GET")),
    );
    if class != RouteClass::PublicReads {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type"),
        );
        headers.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
    }
}

/// Adds no-store for auth and private payloads and always sends nosniff.
fn add_response_headers(response: &mut Response, class: RouteClass, has_authorization: bool) {
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    let auth_surface = class != RouteClass::PublicReads || has_authorization;
    if auth_surface {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

/// Rate-limit key: the socket peer, or the forwarded chain right-to-left when
/// the immediate peer is a trusted proxy. Missing ConnectInfo (direct service
/// calls in tests) keys everything under an unspecified IPv4.
fn request_client_ip(state: &AppState, request: &Request) -> IpAddr {
    let peer = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    let forwarded = request
        .headers()
        .get(X_FORWARDED_FOR)
        .and_then(|value| value.to_str().ok());
    client_ip::client_ip(peer, forwarded, &state.config.trusted_proxies)
}
