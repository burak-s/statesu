use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::error::Error;
use crate::http::rate_limit::RateLimiter;
use crate::http::static_files::fallback as docs_fallback;

pub mod auth;
pub mod blog;
pub mod client_ip;
pub mod error;
pub mod json_body;
pub mod post;
pub mod public;
pub mod rate_limit;
pub mod static_files;

pub use json_body::StrictJson;

/// Shared application services injected through the router state only.
#[derive(Clone)]
pub struct AppState {
    pub config: Config,
    pub auth_service: crate::auth::service::AuthService,
    pub blogs: crate::blog::service::BlogService,
    pub posts: crate::post::service::PostService,
    pub assets: Arc<dyn static_files::StaticAssets>,
    /// Verification mail delivery; never called outside register/resend.
    pub mailer: Arc<dyn crate::mail::Mailer>,
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

/// JSON body limits: auth endpoints accept 16 KiB, blog/post endpoints
/// accept 256 KiB so imported Markdown files fit.
const AUTH_BODY_LIMIT: usize = 16 * 1024;
const CMS_BODY_LIMIT: usize = 256 * 1024;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Assembles the production router: auth + management routes guarded by CORS
/// and rate limits, public delivery routes with per-blog origins, plus an
/// embedded docs fallback returning 404/405.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/verify", post(auth::verify))
        .route("/auth/resend", post(auth::resend))
        .route("/auth/pats", post(auth::create_pat).get(auth::list_pats))
        .route("/auth/pats/{pat_id}", delete(auth::delete_pat))
        .route("/auth/sessions", delete(auth::logout_all))
        .route("/blogs", post(blog::create).get(blog::list))
        .route("/blogs/{blog_id}", patch(blog::update).delete(blog::delete))
        .route("/blogs/{blog_id}/origins", put(blog::set_origins))
        .route("/blogs/{blog_id}/posts", post(post::create).get(post::list))
        .route(
            "/blogs/{blog_id}/posts/{post_id}",
            get(post::get).patch(post::update).delete(post::delete),
        )
        .route("/public/blogs/{blog_slug}", get(public::blog))
        .route("/public/blogs/{blog_slug}/posts", get(public::posts))
        .route(
            "/public/blogs/{blog_slug}/posts/{post_slug}",
            get(public::post),
        )
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
///
/// Borrowed values cross no `await`: `http::Request` is not `Sync`, so the
/// request is only inspected before the first await; method, path, origin and
/// any per-blog origins are owned afterwards.
pub async fn request_guard(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let access_control_method = if method == Method::OPTIONS {
        request
            .headers()
            .get(header::ACCESS_CONTROL_REQUEST_METHOD)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| Method::from_bytes(value.as_bytes()).ok())
    } else {
        None
    };
    let is_preflight = method == Method::OPTIONS && access_control_method.is_some();
    let class = if is_preflight {
        class_for_method(
            access_control_method.as_ref().unwrap_or(&Method::GET),
            &path,
        )
    } else {
        class_for_method(&method, &path)
    };
    let origin = request.headers().get(header::ORIGIN).cloned();
    let client = request_client_ip(&state, &request);
    let has_authorization = request.headers().contains_key(header::AUTHORIZATION);

    // Delivery CORS is scoped to the blog in the path: only origins the blog
    // owner recorded may call it from a browser.
    let public_origins = if class == RouteClass::PublicDelivery {
        public_blog_origins(&state, &path).await
    } else {
        Vec::new()
    };

    // Preflight short-circuit: OPTIONS with Origin and an
    // Access-Control-Request-Method is answered here.
    if is_preflight && let Some(origin) = origin {
        let mut response = StatusCode::NO_CONTENT.into_response();
        if class == RouteClass::PublicDelivery {
            apply_public_cors(public_origins, Some(&origin), &mut response, true);
        } else {
            apply_cors(&state, Some(&origin), &mut response, true);
        }
        return response;
    }

    // Unauthenticated write endpoints (register/login/verify/resend) pin
    // browser origins: any same-kind cross-site write is rejected before
    // limiter or handler run. Absent Origin (curl, wget) stays allowed.
    // The public URL and the dashboard allowlist both count as same-site.
    if let Some(origin) = origin.as_ref()
        && class == RouteClass::SensitiveAuth
        && !is_preflight
        && !state.config.origin_allowed(origin.to_str().unwrap_or(""))
    {
        return Error::Forbidden.into_response();
    }

    let limiter = match class {
        RouteClass::SensitiveAuth => &state.limiters.auth,
        _ => &state.limiters.general,
    };
    let mut response = match limiter.take_slot(client) {
        Ok(()) => next.run(request).await,
        Err(retry_after_secs) => Error::RateLimited { retry_after_secs }.into_response(),
    };
    if class == RouteClass::PublicDelivery {
        apply_public_cors(public_origins, origin.as_ref(), &mut response, false);
    } else {
        apply_cors(&state, origin.as_ref(), &mut response, false);
    }
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
    match *method {
        Method::POST | Method::PATCH | Method::PUT => {
            if path.starts_with("/auth") {
                Some(AUTH_BODY_LIMIT)
            } else if path.starts_with("/blogs") {
                Some(CMS_BODY_LIMIT)
            } else {
                None
            }
        }
        _ => None,
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
    /// Public delivery reads; CORS is scoped to the blog's recorded origins.
    PublicDelivery,
    /// register/login: CORS allowlist plus a tight quota.
    SensitiveAuth,
    /// Other auth routes and management writes: CORS allowlist, general quota.
    Restricted,
}

fn class_for_method(method: &Method, path: &str) -> RouteClass {
    let is_read = method == Method::GET || method == Method::HEAD;
    if is_read && path.starts_with("/public/") {
        RouteClass::PublicDelivery
    } else if matches!(
        path,
        "/auth/register" | "/auth/login" | "/auth/verify" | "/auth/resend"
    ) {
        RouteClass::SensitiveAuth
    } else {
        RouteClass::Restricted
    }
}

/// Applies the CORS policy for management and auth routes. These require an
/// exact allowlist match against the configured dashboard origins.
/// Credentialed CORS is deliberately never enabled.
fn apply_cors(
    state: &AppState,
    origin: Option<&HeaderValue>,
    response: &mut Response,
    preflight: bool,
) {
    let Some(origin) = origin else {
        return;
    };
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
    let headers = response.headers_mut();
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    if preflight {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD, POST, DELETE, PATCH, PUT"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type, X-CSRF-Token"),
        );
        headers.insert(
            header::ACCESS_CONTROL_MAX_AGE,
            HeaderValue::from_static("600"),
        );
    }
}

/// Exact origins recorded for the blog named in a `/public/blogs/{slug}`
/// path; unknown blogs read as empty so no origin ever matches.
async fn public_blog_origins(state: &AppState, path: &str) -> Vec<String> {
    let slug = public_blog_slug(path);
    match slug {
        Some(slug) => state.blogs.origins_for_slug(slug).await.unwrap_or_default(),
        None => Vec::new(),
    }
}

/// `/public/blogs/{blog_slug}` optionally followed by `/posts/...`; returns
/// the slug segment when the path has that shape.
fn public_blog_slug(path: &str) -> Option<&str> {
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());
    if segments.next() != Some("public") || segments.next() != Some("blogs") {
        return None;
    }
    segments.next()
}

/// Applies per-blog delivery CORS. Origins must match exactly; responses
/// always advertise `Vary: Origin` so shared caches never reuse a decision
/// made for another origin.
fn apply_public_cors(
    allowed_origins: Vec<String>,
    origin: Option<&HeaderValue>,
    response: &mut Response,
    preflight: bool,
) {
    let headers = response.headers_mut();
    headers.append(header::VARY, HeaderValue::from_static("Origin"));
    let Some(origin) = origin else {
        return;
    };
    let Ok(origin_str) = origin.to_str() else {
        return;
    };
    if !allowed_origins.iter().any(|allowed| allowed == origin_str) {
        return;
    }
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    if preflight {
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, HEAD"),
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
    let auth_surface = class != RouteClass::PublicDelivery || has_authorization;
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
        .map(|connect| connect.0.ip())
        .unwrap_or(std::net::Ipv4Addr::UNSPECIFIED.into());
    let forwarded = request
        .headers()
        .get(X_FORWARDED_FOR)
        .and_then(|value| value.to_str().ok());
    client_ip::client_ip(peer, forwarded, &state.config.trusted_proxies)
}
