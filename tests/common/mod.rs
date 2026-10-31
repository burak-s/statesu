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
use statesu::bootstrap::{App, build_app_with_mailer};
use statesu::clock::TestClock;
use statesu::config::{Config, IpNet};
use statesu::http::static_files::{StaticAssets, StaticFile};
use statesu::mail::Mailer;
use tempfile::TempDir;
use tower::ServiceExt;

/// The configured public origin for tests: the dashboard origin, which is
/// also the CORS allowlist entry.
pub fn public_url() -> statesu::config::PublicUrl {
    statesu::config::PublicUrl::parse(ALLOWED_ORIGIN).expect("test public url")
}

pub const EMAIL: &str = "user@example.com";
pub const PASSWORD: &str = "correct horse 7";
pub const NOW: i64 = 1_700_000_000;
pub const ALLOWED_ORIGIN: &str = "https://allowed.example";

pub struct TestApp {
    pub app: App,
    pub clock: Arc<TestClock>,
    /// Message sink for the mailer; production code never inspects it.
    pub sent_mail: Arc<std::sync::Mutex<Vec<SentMail>>>,
    _temp: TempDir,
}

/// One recorded verification mail.
#[derive(Debug, Clone)]
pub struct SentMail {
    pub to: String,
    pub link: String,
}

/// Recording mailer: implements the production trait without any network.
#[derive(Clone, Default)]
pub struct TestMailer {
    messages: Arc<std::sync::Mutex<Vec<SentMail>>>,
}

impl TestMailer {
    pub fn shared() -> (TestMailer, Arc<std::sync::Mutex<Vec<SentMail>>>) {
        let mailer = TestMailer::default();
        let sink = mailer.messages.clone();
        (mailer, sink)
    }

    pub fn links_for(&self, to: &str) -> Vec<String> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .filter(|mail| mail.to == to)
            .map(|mail| mail.link.clone())
            .collect()
    }
}

impl Mailer for TestMailer {
    fn send_verification<'a>(
        &'a self,
        to: &'a str,
        link: &'a str,
    ) -> statesu::mail::MailFuture<'a> {
        let mail = SentMail {
            to: to.to_owned(),
            link: link.to_owned(),
        };
        self.messages.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    }
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
        let (mailer, sent_mail) = TestMailer::shared();
        let app = build_app_with_mailer(
            config,
            clock.clone(),
            Arc::new(TestAssets::standard()),
            Arc::new(mailer),
        )
        .await
        .expect("build app");
        TestApp {
            app,
            clock,
            sent_mail,
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

/// The verification link the mailer delivered for `email`.
pub async fn verification_link(app: &TestApp, email: &str) -> String {
    app.sent_mail
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|mail| mail.to == statesu::crypto::email::normalize_email(email))
        .map(|mail| mail.link.clone())
        .expect("a verification mail was sent")
}

/// Registers and confirms the address, ready for login.
pub async fn register_and_verify(app: &TestApp, email: &str, password: &str) -> TestResponse {
    let response = register(app, email, password).await;
    assert_eq!(response.status, StatusCode::ACCEPTED, "{}", response.text());
    let link = verification_link(app, email).await;
    let token = link
        .rsplit("token=")
        .next()
        .expect("token in link")
        .to_owned();
    let verify = app
        .send(json_request(
            Method::POST,
            "/auth/verify",
            json!({ "token": token }),
        ))
        .await;
    assert_eq!(verify.status, StatusCode::OK, "{}", verify.text());
    response
}

pub async fn login(app: &TestApp, email: &str, password: &str) -> TestResponse {
    app.send(json_request(
        Method::POST,
        "/auth/login",
        json!({ "email": email, "password": password }),
    ))
    .await
}

/// The two `Set-Cookie` values of a successful login, joined for the
/// `Cookie` request header plus the CSRF cookie value for the header check.
pub async fn login_session(app: &TestApp, email: &str, password: &str) -> (String, String) {
    let response = login(app, email, password).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    session_pair(&response)
}

/// Extracts cookie-pair + CSRF token from a login response.
pub fn session_pair(response: &TestResponse) -> (String, String) {
    let mut session = String::new();
    let mut csrf_value = String::new();
    for (i, raw) in response
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .enumerate()
    {
        if raw.starts_with("__Host-statesu_session=") {
            assert_eq!(i, 0);
            let value = raw.split(';').next().unwrap().to_owned();
            assert!(raw.contains("HttpOnly"), "session cookie is HttpOnly");
            session = value;
        } else if raw.starts_with("__Host-statesu_csrf=") {
            csrf_value = raw
                .split(';')
                .next()
                .unwrap()
                .split('=')
                .nth(1)
                .unwrap()
                .to_owned();
        }
    }
    assert!(!session.is_empty(), "session cookie missing");
    assert!(!csrf_value.is_empty(), "csrf cookie missing");
    (
        format!("{session}; __Host-statesu_csrf={csrf_value}"),
        csrf_value,
    )
}

/// Creates a personal access token over a session; returns its secret.
pub async fn create_pat(app: &TestApp, session_cookie: &str, csrf: &str, write: bool) -> String {
    let request = json_request(
        Method::POST,
        "/auth/pats",
        json!({ "name": "tests", "write": write }),
    );
    let (mut parts, body) = request.into_parts();
    parts
        .headers
        .insert("cookie", cookie_header(session_cookie));
    parts.headers.insert(
        "x-csrf-token",
        HeaderValue::from_str(csrf).expect("csrf header"),
    );
    let response = app.send(Request::from_parts(parts, body)).await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
    response.json()["secret"]
        .as_str()
        .expect("secret")
        .to_owned()
}

pub fn cookie_header(session_cookie: &str) -> HeaderValue {
    HeaderValue::from_str(session_cookie).expect("cookie header")
}

/// Writes `Cookie: session; csrf` and `X-CSRF-Token` onto a request.
pub fn session_authorized(
    mut request: Request<Body>,
    session_cookie: &str,
    csrf: &str,
) -> Request<Body> {
    {
        let headers = request.headers_mut();
        headers.insert("cookie", cookie_header(session_cookie));
        headers.insert(
            "x-csrf-token",
            HeaderValue::from_str(csrf).expect("csrf header"),
        );
    }
    request
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

/// Registers, verifies and mints a read/write PAT for bearer-style tests.
pub async fn register_token(app: &TestApp, email: &str, password: &str) -> String {
    register_and_verify(app, email, password).await;
    let (session_cookie, csrf) = login_session(app, email, password).await;
    create_pat(app, &session_cookie, &csrf, true).await
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
        resend_api_key: "test-resend-key".to_owned(),
        mail_from: "Statesu <no-reply@test.example>".to_owned(),
        public_url: public_url(),
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
