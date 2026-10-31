use std::borrow::Cow;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::http::AppState;

/// The only polymorphic seam besides `Clock`: the production implementation
/// wraps the embedded Svelte build, while tests inject an in-memory map.
pub trait StaticAssets: Send + Sync {
    fn get(&self, path: &str) -> Option<StaticFile>;
}

/// One embedded documentation asset.
#[derive(Debug, Clone)]
pub struct StaticFile {
    /// Docs payloads are small; no streaming transport is needed.
    pub body: Cow<'static, [u8]>,
    pub mime: String,
}

/// Wraps the build output embedded at compile time.
pub struct EmbeddedDocs;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets/dist"]
struct DocFiles;

impl StaticAssets for EmbeddedDocs {
    fn get(&self, path: &str) -> Option<StaticFile> {
        DocFiles::get(path).map(|asset| StaticFile {
            body: Cow::Owned(asset.data.to_vec()),
            mime: mime_for(path),
        })
    }
}

fn mime_for(path: &str) -> String {
    let mime = mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string();
    let is_text = mime.starts_with("text/")
        || matches!(
            mime.as_str(),
            "application/javascript" | "application/json" | "image/svg+xml"
        );
    if is_text {
        format!("{mime}; charset=utf-8")
    } else {
        mime
    }
}

enum CacheMode {
    NoCache,
    Immutable,
    Default,
}

/// Serves the docs root plus embedded assets: unknown paths are 404 (no SPA
/// fallback), non-GET/HEAD requests are 405, and HEAD responses carry headers
/// with an empty body.
pub async fn fallback(State(state): State<AppState>, request: Request) -> Response {
    let head_request = request.method() == Method::HEAD;
    if request.method() != Method::GET && !head_request {
        return not_allowed_response();
    }
    let raw_path = request.uri().path().trim_start_matches('/');
    let index = raw_path.is_empty() || raw_path == "index.html";
    let lookup_path = if index {
        "index.html".to_owned()
    } else {
        match clean_asset_path(raw_path) {
            Some(path) => path,
            None => return not_found_response(),
        }
    };
    let Some(asset) = state.assets.get(&lookup_path) else {
        return not_found_response();
    };
    let cache_mode = if index {
        CacheMode::NoCache
    } else if raw_path.starts_with("_app/immutable/") {
        CacheMode::Immutable
    } else {
        CacheMode::Default
    };
    let content_type = HeaderValue::from_str(&asset.mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type);
    builder = match cache_mode {
        CacheMode::NoCache => builder.header(header::CACHE_CONTROL, "no-cache"),
        CacheMode::Immutable => {
            builder.header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        }
        CacheMode::Default => builder,
    };
    let body = if head_request {
        Vec::new()
    } else {
        asset.body.into_owned()
    };
    let Ok(mut response) = builder.body(Body::from(body)) else {
        return internal_response();
    };
    apply_docs_security_headers(&mut response, &state);
    response
}

/// Rejects traversal and hidden-path tricks before the asset map lookup.
fn clean_asset_path(path: &str) -> Option<String> {
    if path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return None;
    }
    Some(path.to_owned())
}

fn apply_docs_security_headers(response: &mut Response, state: &AppState) {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&state.csp) {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
}

fn not_found_response() -> Response {
    plain_status_response(StatusCode::NOT_FOUND, None, "Not Found")
}

fn not_allowed_response() -> Response {
    plain_status_response(
        StatusCode::METHOD_NOT_ALLOWED,
        Some("GET, HEAD"),
        "Method Not Allowed",
    )
}

fn internal_response() -> Response {
    plain_status_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        None,
        "Internal Server Error",
    )
}

fn plain_status_response(
    status: StatusCode,
    allow: Option<&'static str>,
    body: &'static str,
) -> Response {
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8");
    if let Some(allow) = allow {
        builder = builder.header(header::ALLOW, allow);
    }
    let Ok(mut response) = builder.body(Body::from(body)) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
