#![allow(dead_code)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, HeaderValue, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use statesu::auth::turnstile::TurnstileVerifier;
use statesu::bootstrap::{App, build_app_with_turnstile};
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

/// Stub human-verification: accepts exactly the expected token, rejects
/// anything else, without touching the network.
pub struct TestTurnstile {
    required: &'static str,
}

impl TestTurnstile {
    pub const TOKEN: &'static str = "good-turnstile-token";

    pub fn boxed() -> Arc<dyn TurnstileVerifier> {
        Arc::new(TestTurnstile {
            required: Self::TOKEN,
        })
    }
}

impl TurnstileVerifier for TestTurnstile {
    fn verify<'a>(
        &'a self,
        token: &'a str,
        _remote_ip: Option<&'a str>,
    ) -> statesu::auth::turnstile::TurnstileFuture<'a> {
        let result = if token == self.required {
            Ok(())
        } else {
            Err(statesu::error::Error::TurnstileFailed)
        };
        Box::pin(async move { result })
    }
}

/// Recorded storage events, in exact order: `put:<key>` and `delete:<key>`
/// entries let assertions check what the service did to the bucket and
/// when it did it.
pub type StorageEvents = Arc<std::sync::Mutex<Vec<String>>>;

/// Recording storage factory: no network, one shared in-memory bucket.
#[derive(Clone, Default)]
pub struct TestStorage {
    /// `"account:bucket:access_key"` per verification call.
    pub verified: Arc<std::sync::Mutex<Vec<String>>>,
    pub fail_verify: Arc<std::sync::atomic::AtomicBool>,
    /// When set, the next `put` calls on stores fail, in order: the n-th
    /// write rejects, every later one too. Failure injection for the
    /// durability paths.
    pub put_failures_remaining: Arc<std::sync::atomic::AtomicI64>,
    /// When set, the first `incoming` puts succeed and every later one
    /// fails: a real half-write (some objects landed, the upload died).
    pub put_fails_after: Arc<std::sync::atomic::AtomicI64>,
    /// Counted successful puts since `put_fails_after` was armed.
    put_successes: Arc<std::sync::atomic::AtomicI64>,
    /// When set, deletes fail the same way.
    pub delete_failures_remaining: Arc<std::sync::atomic::AtomicI64>,
    pub events: StorageEvents,
    /// `account_id/bucket/access_key_id` per store_for call, in order.
    credentials: Arc<std::sync::Mutex<Vec<String>>>,
    objects: Arc<std::sync::Mutex<HashMap<String, Vec<u8>>>>,
}

impl TestStorage {
    pub fn shared() -> Arc<TestStorage> {
        Arc::new(TestStorage::default())
    }

    pub fn object(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.lock().unwrap().get(key).cloned()
    }

    pub fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    /// Makes the next `incoming` puts fail.
    pub fn fail_puts(&self, incoming: i64) {
        self.put_failures_remaining
            .store(incoming, std::sync::atomic::Ordering::SeqCst);
    }

    /// Arms the half-write scenario: the first `incoming` puts succeed,
    /// then everything rejects.
    pub fn fail_puts_after(&self, incoming: i64) {
        self.put_fails_after
            .store(incoming, std::sync::atomic::Ordering::SeqCst);
        self.put_successes
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }

    /// Makes the next `incoming` deletes fail.
    pub fn fail_deletes(&self, incoming: i64) {
        self.delete_failures_remaining
            .store(incoming, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Store view into the shared in-memory bucket, driven through the seam.
/// Store view into the shared in-memory bucket, driven through the seam.
struct RecordingStore {
    objects: Arc<std::sync::Mutex<HashMap<String, Vec<u8>>>>,
    events: StorageEvents,
    put_failures_remaining: Arc<std::sync::atomic::AtomicI64>,
    put_fails_after: Arc<std::sync::atomic::AtomicI64>,
    put_successes: Arc<std::sync::atomic::AtomicI64>,
    delete_failures_remaining: Arc<std::sync::atomic::AtomicI64>,
}

impl RecordingStore {
    fn take(&self, counter: &std::sync::atomic::AtomicI64) -> bool {
        let mut remaining = counter.load(std::sync::atomic::Ordering::SeqCst);
        while remaining > 0 {
            match counter.compare_exchange(
                remaining,
                remaining - 1,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            ) {
                Ok(_) => return true,
                Err(actual) => remaining = actual,
            }
        }
        false
    }
}

impl statesu::storage::ObjectStore for RecordingStore {
    fn put<'a>(&'a self, key: &'a str, body: &'a [u8]) -> statesu::storage::StoreFuture<'a> {
        self.events.lock().unwrap().push(format!("put:{key}"));
        let threshold = self
            .put_fails_after
            .load(std::sync::atomic::Ordering::SeqCst);
        if threshold > 0 {
            let successes = self
                .put_successes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if successes >= threshold {
                return Box::pin(async {
                    Err(statesu::error::Error::Internal(
                        statesu::error::InternalError::new(std::io::Error::other(
                            "injected half-write failure",
                        )),
                    ))
                });
            }
        }
        if self.take(&self.put_failures_remaining) {
            return Box::pin(async {
                Err(statesu::error::Error::Internal(
                    statesu::error::InternalError::new(std::io::Error::other(
                        "injected put failure",
                    )),
                ))
            });
        }
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_owned(), body.to_vec());
        Box::pin(async { Ok(()) })
    }

    fn delete<'a>(&'a self, key: &'a str) -> statesu::storage::StoreFuture<'a> {
        self.events.lock().unwrap().push(format!("delete:{key}"));
        if self.take(&self.delete_failures_remaining) {
            return Box::pin(async {
                Err(statesu::error::Error::Internal(
                    statesu::error::InternalError::new(std::io::Error::other(
                        "injected delete failure",
                    )),
                ))
            });
        }
        self.objects.lock().unwrap().remove(key);
        Box::pin(async { Ok(()) })
    }
}

impl TestStorage {
    /// The exact connection material each `store_for` built, in order:
    /// `account_id/bucket/access_key_id`. Cleanup engineered on snapshot
    /// vs current config depends on this log.
    pub fn used_credentials(&self) -> Vec<String> {
        self.credentials.lock().unwrap().clone()
    }
}

impl statesu::storage::StorageFactory for TestStorage {
    fn verify_connection<'a>(
        &'a self,
        connection: &'a statesu::storage::BucketConnection,
        public_base_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), statesu::error::Error>> + Send + 'a>> {
        self.verified.lock().unwrap().push(format!(
            "{}:{}:{}",
            connection.account_id, connection.bucket, connection.access_key_id
        ));
        let fail = self.fail_verify.load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            let _ = public_base_url;
            if fail {
                Err(statesu::error::Error::ConnectionInvalid)
            } else {
                Ok(())
            }
        })
    }

    fn store_for(
        &self,
        connection: &statesu::storage::BucketConnection,
    ) -> Result<Arc<dyn statesu::storage::ObjectStore>, statesu::error::Error> {
        self.credentials.lock().unwrap().push(format!(
            "{}/{}/{}",
            connection.account_id, connection.bucket, connection.access_key_id
        ));
        Ok(Arc::new(RecordingStore {
            objects: self.objects.clone(),
            events: self.events.clone(),
            put_failures_remaining: self.put_failures_remaining.clone(),
            put_fails_after: self.put_fails_after.clone(),
            put_successes: self.put_successes.clone(),
            delete_failures_remaining: self.delete_failures_remaining.clone(),
        }))
    }
}

impl TestApp {
    pub async fn new() -> TestApp {
        Self::build(|_| {}).await
    }

    /// An app with a Turnstile verifier injected, whatever the config says.
    pub async fn with_turnstile(configure: impl FnOnce(&mut Config)) -> TestApp {
        Self::build_full(configure, Some(TestTurnstile::boxed())).await
    }

    pub async fn with_limits(general: u64, auth: u64) -> TestApp {
        Self::build(|config| {
            config.rate_limit_rpm = general;
            config.auth_rate_limit_rpm = auth;
        })
        .await
    }

    pub async fn build(configure: impl FnOnce(&mut Config)) -> TestApp {
        Self::build_full(configure, None).await
    }

    /// An app whose storage seam is the recording in-memory factory; the
    /// shared bucket stays accessible to the caller for object assertions.
    pub async fn with_storage() -> (TestApp, Arc<TestStorage>) {
        let storage = TestStorage::shared();
        let app = Self::build_storage(|_| {}, storage.clone()).await;
        (app, storage)
    }

    /// Same seam, plus a server-owned R2 default bucket for upload tests.
    pub async fn with_server_storage() -> (TestApp, Arc<TestStorage>) {
        let storage = TestStorage::shared();
        let app = Self::build_storage(
            |config| config.r2_default = Some(Self::test_server_storage()),
            storage.clone(),
        )
        .await;
        (app, storage)
    }

    /// The recording seam with arbitrary config shaping.
    pub async fn with_configured_storage(
        configure: impl FnOnce(&mut Config),
    ) -> (TestApp, Arc<TestStorage>) {
        let storage = TestStorage::shared();
        let app = Self::build_storage(configure, storage.clone()).await;
        (app, storage)
    }

    pub fn test_server_storage() -> statesu::config::R2Default {
        statesu::config::R2Default {
            account_id: "abcdef0123456789abcdef0123456789".to_owned(),
            bucket: "statesu-bucket".to_owned(),
            prefix: Some("statesu".to_owned()),
            access_key_id: "server-key".to_owned(),
            secret_key: "server-secret".to_owned(),
            public_base_url: "https://media.test.example".to_owned(),
        }
    }

    async fn build_storage(
        configure: impl FnOnce(&mut Config),
        storage: Arc<TestStorage>,
    ) -> TestApp {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut config = test_config(temp.path().join("statesu.db"));
        configure(&mut config);
        let clock = Arc::new(TestClock::at_start(NOW));
        let (mailer, sent_mail) = TestMailer::shared();
        let app = statesu::bootstrap::build_app_with_storage(
            config,
            clock.clone(),
            Arc::new(TestAssets::standard()),
            Arc::new(mailer),
            None,
            storage,
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

    async fn build_full(
        configure: impl FnOnce(&mut Config),
        turnstile: Option<Arc<dyn TurnstileVerifier>>,
    ) -> TestApp {
        let temp = tempfile::tempdir().expect("temp dir");
        let mut config = test_config(temp.path().join("statesu.db"));
        configure(&mut config);
        let clock = Arc::new(TestClock::at_start(NOW));
        let (mailer, sent_mail) = TestMailer::shared();
        let app = build_app_with_turnstile(
            config,
            clock.clone(),
            Arc::new(TestAssets::standard()),
            Arc::new(mailer),
            turnstile,
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

pub async fn set_appearance(
    app: &TestApp,
    token: &str,
    blog_id: &str,
    body: Value,
) -> TestResponse {
    let request = json_request(Method::PUT, &format!("/blogs/{blog_id}/appearance"), body);
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

/// `authed` with a JSON body: POST/PATCH helpers share this path.
pub async fn authed_json(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Value,
) -> TestResponse {
    let request = json_request(method, path, body);
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
        // Tight enough that quota tests finish fast; quota tests that need a
        // different budget shrink it further through `build(configure)`.
        max_posts_per_user: 64,
        max_total_content_bytes: 2 * 1024 * 1024,
        log_json: false,
        turnstile_secret: None,
        media_key: [11u8; 32],
        r2_default: None,
        max_media_bytes: 64 * 1024,
        max_total_media_bytes: 2 * 1024 * 1024,
        max_media_per_user: 64,
    }
}

pub struct TestAssets {
    files: HashMap<&'static str, (&'static [u8], &'static str)>,
}

impl TestAssets {
    pub fn standard() -> TestAssets {
        let mut files = HashMap::new();
        for path in [
            "index.html",
            "docs/integration.html",
            "dashboard.html",
            "verify.html",
            "cli/authorize.html",
            "demo/blog/index.html",
            "demo/blog/index/index.html",
        ] {
            files.insert(
                path,
                (
                    b"<!doctype html><html><body>docs</body></html>".as_slice(),
                    "text/html; charset=utf-8",
                ),
            );
        }
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
        files.insert(
            "embed/v1.js",
            (
                b"/* Statesu embed v1 */".as_slice(),
                "text/javascript; charset=utf-8",
            ),
        );
        files.insert(
            "embed/v2.js",
            (
                b"/* Statesu embed v2 */".as_slice(),
                "text/javascript; charset=utf-8",
            ),
        );
        files.insert(
            "llms.txt",
            (
                include_bytes!("../../web/static/llms.txt").as_slice(),
                "text/plain; charset=utf-8",
            ),
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
