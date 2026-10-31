#![allow(dead_code)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use statesu::bootstrap::{App, build_app};
use statesu::clock::TestClock;
use statesu::config::{Config, IpNet};
use statesu::http::static_files::{StaticAssets, StaticFile};
use tempfile::TempDir;
use tower::ServiceExt;

pub const EMAIL: &str = "user@example.com";
pub const PASSWORD: &str = "correct horse 7";
pub const NOW: i64 = 1_700_000_000;
pub const ALLOWED_ORIGIN: &str = "https://allowed.example";

pub struct TestApp {
    pub app: App,
    pub clock: Arc<TestClock>,
    _temp: TempDir,
}

impl TestApp {
    pub async fn new() -> TestApp {
        Self::build(|_| {}).await
    }

    pub async fn with_limits(general: u64, auth: u64) -> TestApp {
        Self::build(|config| {
            config.rate_limit_rpm = general;
            config.auth_rate_limit_rpm = auth;
        })
        .await
    }

    pub async fn build(configure: impl FnOnce(&mut Config)) -> TestApp {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut config = test_config(temp.path().join("statesu.db"));
        configure(&mut config);
        let clock = Arc::new(TestClock::at_start(NOW));
        let app = build_app(config, clock.clone(), Arc::new(TestAssets::standard()))
            .await
            .expect("build app");
        TestApp {
            app,
            clock,
            _temp: temp,
        }
    }

    pub fn router(&self) -> Router {
        self.app.router.clone()
    }

    pub async fn send(&self, request: Request<Body>) -> TestResponse {
        self.send_inner(request).await
    }

    pub async fn send_as(&self, mut request: Request<Body>, peer: IpAddr) -> TestResponse {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer, 40000)));
        self.send_inner(request).await
    }

    async fn send_inner(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router()
            .oneshot(request)
            .await
            .expect("router response");
        let status = response.status();
        let headers = response.headers().clone();
        let body = response
            .into_body()
            .collect()
            .await
            .expect("response body")
            .to_bytes();
        TestResponse {
            status,
            headers,
            body,
        }
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: axum::body::Bytes,
}

impl TestResponse {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }

    pub fn assert_error(&self, status: StatusCode, message: &str) {
        assert_eq!(self.status, status, "body: {}", self.text());
        assert_eq!(self.json(), json!({ "error": message }));
    }

    pub fn status_check(&self, status: StatusCode) -> &Self {
        assert_eq!(self.status, status, "body: {}", self.text());
        self
    }
}

pub fn empty(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request")
}

pub fn json_request(method: Method, path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

pub fn bearer(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("Bearer {token}")).expect("bearer header")
}

pub async fn register(app: &TestApp, email: &str, password: &str) -> TestResponse {
    app.send(json_request(
        Method::POST,
        "/auth/register",
        json!({ "email": email, "password": password }),
    ))
    .await
}

pub async fn register_token(app: &TestApp, email: &str, password: &str) -> String {
    let response = register(app, email, password).await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
    response.json()["token"].as_str().expect("token").to_owned()
}

pub async fn login(app: &TestApp, email: &str, password: &str) -> TestResponse {
    app.send(json_request(
        Method::POST,
        "/auth/login",
        json!({ "email": email, "password": password }),
    ))
    .await
}

pub async fn create_blog(app: &TestApp, token: &str, name: &str, slug: &str) -> TestResponse {
    let request = json_request(
        Method::POST,
        "/blogs",
        json!({ "name": name, "slug": slug }),
    );
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    app.send(Request::from_parts(parts, body)).await
}

pub async fn create_post(app: &TestApp, token: &str, blog_id: &str, body: Value) -> TestResponse {
    let request = json_request(Method::POST, &format!("/blogs/{blog_id}/posts"), body);
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    app.send(Request::from_parts(parts, body)).await
}

pub async fn patch_post(
    app: &TestApp,
    token: &str,
    blog_id: &str,
    post_id: &str,
    body: Value,
) -> TestResponse {
    let request = json_request(
        Method::PATCH,
        &format!("/blogs/{blog_id}/posts/{post_id}"),
        body,
    );
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    app.send(Request::from_parts(parts, body)).await
}

pub async fn set_origins(
    app: &TestApp,
    token: &str,
    blog_id: &str,
    origins: Value,
) -> TestResponse {
    let request = json_request(
        Method::PUT,
        &format!("/blogs/{blog_id}/origins"),
        json!({ "origins": origins }),
    );
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    app.send(Request::from_parts(parts, body)).await
}

pub async fn authed(app: &TestApp, method: Method, path: &str, token: &str) -> TestResponse {
    let request = empty(method, path);
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    app.send(Request::from_parts(parts, body)).await
}

pub fn test_config(db_path: std::path::PathBuf) -> Config {
    Config {
        addr: "127.0.0.1:0".to_owned(),
        db_path: db_path.to_string_lossy().into_owned(),
        email_key: [7u8; 32],
        email_hmac_key: vec![9u8; 32],
        jwt_secret: vec![11u8; 32],
        jwt_audience: "statesu-api".to_owned(),
        allowed_origins: vec![ALLOWED_ORIGIN.to_owned()],
        trusted_proxies: vec![IpNet::parse("10.0.0.0/8").expect("trusted proxy")],
        rate_limit_rpm: 10_000,
        auth_rate_limit_rpm: 10_000,
        max_concurrency: 64,
        argon2_concurrency: 1,
        log_json: false,
    }
}

pub struct TestAssets {
    files: HashMap<&'static str, (&'static [u8], &'static str)>,
}

impl TestAssets {
    pub fn standard() -> TestAssets {
        let mut files = HashMap::new();
        files.insert(
            "index.html",
            (
                b"<!doctype html><html><body>docs</body></html>".as_slice(),
                "text/html; charset=utf-8",
            ),
        );
        files.insert(
            "_app/immutable/app.js",
            (
                b"console.log('docs');".as_slice(),
                "text/javascript; charset=utf-8",
            ),
        );
        files.insert(
            "favicon.svg",
            (b"<svg></svg>".as_slice(), "image/svg+xml; charset=utf-8"),
        );
        TestAssets { files }
    }
}

impl StaticAssets for TestAssets {
    fn get(&self, path: &str) -> Option<StaticFile> {
        self.files.get(path).map(|(body, mime)| StaticFile {
            body: Cow::Borrowed(*body),
            mime: (*mime).to_owned(),
        })
    }
}
