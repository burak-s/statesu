mod common;

use std::net::IpAddr;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::json;

use common::{
    ALLOWED_ORIGIN, EMAIL, PASSWORD, TestApp, create_blog, create_post, empty, json_request,
    register_token,
};

fn with_header(mut request: Request<Body>, name: &'static str, value: &str) -> Request<Body> {
    request
        .headers_mut()
        .insert(name, value.parse().expect("header value"));
    request
}

#[tokio::test]
async fn docs_static_contract_keeps_cache_security_and_404_semantics() {
    let app = TestApp::new().await;

    let index = app.send(empty(Method::GET, "/")).await;
    assert_eq!(index.status, StatusCode::OK);
    assert_eq!(
        index.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(index.header("cache-control"), Some("no-cache"));
    assert_eq!(index.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(index.header("x-frame-options"), Some("DENY"));
    assert_eq!(index.header("referrer-policy"), Some("no-referrer"));
    assert!(
        index
            .header("content-security-policy")
            .is_some_and(|value| value.contains("frame-ancestors 'none'"))
    );

    // Cloudflare injects Web Analytics after the response leaves the app.
    // Both its script and beacon must survive the production CSP.
    let csp = index.header("content-security-policy").expect("docs CSP");
    let directive = |name: &str| {
        csp.split(';')
            .map(str::trim)
            .find(|value| value.split_whitespace().next() == Some(name))
            .expect("CSP directive")
            .split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
    };
    assert!(directive("script-src").contains(&"https://static.cloudflareinsights.com"));
    assert!(directive("connect-src").contains(&"https://cloudflareinsights.com"));
    assert!(!directive("script-src").contains(&"*"));
    assert!(!directive("connect-src").contains(&"*"));

    let asset = app.send(empty(Method::GET, "/_app/immutable/app.js")).await;
    assert_eq!(asset.status, StatusCode::OK);
    assert_eq!(
        asset.header("cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        asset.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );

    // The embed script is served like pages: revalidation always applies so
    // hosted sites never render against a stale integration contract.
    let embed = app.send(empty(Method::GET, "/embed/v1.js")).await;
    assert_eq!(embed.status, StatusCode::OK);
    assert_eq!(
        embed.header("content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(embed.header("cache-control"), Some("no-cache"));
    assert_eq!(embed.header("x-content-type-options"), Some("nosniff"));
    let embed_head = app.send(empty(Method::HEAD, "/embed/v1.js")).await;
    assert_eq!(embed_head.status, StatusCode::OK);
    assert!(embed_head.body.is_empty());

    app.send(empty(Method::GET, "/missing"))
        .await
        .status_check(StatusCode::NOT_FOUND);
    let not_allowed = app.send(empty(Method::POST, "/missing")).await;
    assert_eq!(not_allowed.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(not_allowed.header("allow"), Some("GET, HEAD"));

    let head = app.send(empty(Method::HEAD, "/")).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
    assert_eq!(head.header("cache-control"), Some("no-cache"));
}

#[tokio::test]
async fn static_page_aliases_redirect_without_indexing_account_flows() {
    let app = TestApp::new().await;

    for (method, alias, canonical) in [
        (Method::GET, "/index.html?ref=docs", "/?ref=docs"),
        (Method::GET, "/docs/integration/", "/docs/integration"),
        (Method::HEAD, "/docs/integration.html", "/docs/integration"),
        (Method::GET, "/verify/?token=a%2Bb", "/verify?token=a%2Bb"),
        (Method::GET, "/demo/blog", "/demo/blog/"),
        (Method::GET, "/demo/blog/index.html", "/demo/blog/"),
        (Method::GET, "/demo/blog/index", "/demo/blog/index/"),
    ] {
        let response = app.send(empty(method, alias)).await;
        assert_eq!(response.status, StatusCode::PERMANENT_REDIRECT, "{alias}");
        assert_eq!(response.header("location"), Some(canonical));
        assert!(response.body.is_empty());
    }

    for path in [
        "/missing/",
        "/missing.html",
        "/favicon.svg/",
        "/demo/blog/missing/",
    ] {
        app.send(empty(Method::GET, path))
            .await
            .status_check(StatusCode::NOT_FOUND);
    }

    for (path, robots) in [
        ("/", None),
        ("/docs/integration", None),
        ("/demo/blog/", None),
        ("/demo/blog/index/", None),
        ("/dashboard", Some("noindex")),
        ("/verify", Some("noindex")),
        ("/cli/authorize", Some("noindex, nofollow")),
    ] {
        let response = app.send(empty(Method::HEAD, path)).await;
        assert_eq!(response.status, StatusCode::OK, "{path}");
        assert_eq!(response.header("x-robots-tag"), robots, "{path}");
    }

    let llms = app.send(empty(Method::GET, "/llms.txt")).await;
    assert_eq!(llms.status, StatusCode::OK);
    assert_eq!(
        llms.header("content-type"),
        Some("text/plain; charset=utf-8")
    );
    assert!(llms.text().starts_with("# Statesu\n"));
}

#[tokio::test]
async fn public_delivery_never_allows_unknown_origins_or_credentials() {
    let app = TestApp::new().await;
    let request = with_header(
        empty(Method::GET, "/public/blogs/unknown/posts"),
        "origin",
        "https://anywhere.example",
    );
    let response = app.send(request).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert!(response.header("access-control-allow-origin").is_none());
    assert!(
        response
            .header("access-control-allow-credentials")
            .is_none()
    );
}

#[tokio::test]
async fn restricted_cors_requires_an_exact_allowlisted_origin() {
    let app = TestApp::new().await;

    let allowed = with_header(
        json_request(
            Method::POST,
            "/auth/login",
            json!({ "email": EMAIL, "password": PASSWORD }),
        ),
        "origin",
        ALLOWED_ORIGIN,
    );
    let response = app.send(allowed).await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.header("access-control-allow-origin"),
        Some(ALLOWED_ORIGIN)
    );
    assert!(
        response
            .header("vary")
            .is_some_and(|value| value.contains("Origin"))
    );
    assert!(
        response
            .header("access-control-allow-credentials")
            .is_none()
    );

    let denied = with_header(
        json_request(
            Method::POST,
            "/auth/login",
            json!({ "email": EMAIL, "password": PASSWORD }),
        ),
        "origin",
        "https://evil.example",
    );
    let response = app.send(denied).await;
    // Same-site write pinning: cross-site browser writes are refused without
    // ever touching credentials.
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    assert!(response.header("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn cors_preflight_is_classified_by_requested_method() {
    let app = TestApp::new().await;

    let public = Request::builder()
        .method(Method::OPTIONS)
        .uri("/public/blogs/unknown/posts/public")
        .header("origin", "https://anywhere.example")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap();
    let response = app.send(public).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert!(response.header("access-control-allow-origin").is_none());
    assert!(response.header("access-control-allow-methods").is_none());

    let restricted = Request::builder()
        .method(Method::OPTIONS)
        .uri("/auth/login")
        .header("origin", ALLOWED_ORIGIN)
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .body(Body::empty())
        .unwrap();
    let response = app.send(restricted).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(
        response.header("access-control-allow-origin"),
        Some(ALLOWED_ORIGIN)
    );
    assert_eq!(
        response.header("access-control-allow-methods"),
        Some("GET, HEAD, POST, DELETE, PATCH, PUT")
    );
    assert_eq!(
        response.header("access-control-allow-headers"),
        Some("Authorization, Content-Type, X-CSRF-Token")
    );
    assert_eq!(response.header("access-control-max-age"), Some("600"));
}

#[tokio::test]
async fn auth_responses_are_no_store_but_public_reads_are_not() {
    let app = TestApp::new().await;
    let login = login_response(&app).await;
    assert_eq!(login.header("cache-control"), Some("no-store"));
    assert_eq!(login.header("x-content-type-options"), Some("nosniff"));

    let public = app.send(empty(Method::GET, "/public/blogs/none")).await;
    assert!(public.header("cache-control").is_none());
    assert_eq!(public.header("x-content-type-options"), Some("nosniff"));
}

#[tokio::test]
async fn registration_limits_share_ip_budget_across_emails_and_ignore_spoofed_headers() {
    let app = TestApp::new().await;
    let peer: IpAddr = "192.0.2.50".parse().unwrap();
    for index in 0..10 {
        if index > 0 && index % 3 == 0 {
            // Another email and a forged forwarded address cannot buy
            // a fourth registration in the same rolling hour.
            let blocked = with_header(
                json_request(
                    Method::POST,
                    "/auth/register",
                    json!({
                        "email": format!("blocked-{index}@example.com"), "password": PASSWORD
                    }),
                ),
                "x-forwarded-for",
                "8.8.8.8",
            );
            app.send_as(blocked, peer)
                .await
                .assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
            app.clock.advance(3600);
        }
        let response = app
            .send_as(
                json_request(
                    Method::POST,
                    "/auth/register",
                    json!({
                        "email": format!("registration-{index}@example.com"), "password": PASSWORD
                    }),
                ),
                peer,
            )
            .await;
        assert_eq!(response.status, StatusCode::ACCEPTED, "{}", response.text());
    }
    let daily = app
        .send_as(
            json_request(
                Method::POST,
                "/auth/register",
                json!({
                    "email": "daily-blocked@example.com", "password": PASSWORD
                }),
            ),
            peer,
        )
        .await;
    daily.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
    assert_eq!(daily.header("retry-after"), Some("75600"));
    assert_eq!(app.sent_mail.lock().unwrap().len(), 10);

    let other = app
        .send_as(
            json_request(
                Method::POST,
                "/auth/register",
                json!({
                    "email": "other-ip@example.com", "password": PASSWORD
                }),
            ),
            "192.0.2.51".parse().unwrap(),
        )
        .await;
    assert_eq!(other.status, StatusCode::ACCEPTED);
    app.clock.advance(86_400);
    let restored = app
        .send_as(
            json_request(
                Method::POST,
                "/auth/register",
                json!({
                    "email": "next-day@example.com", "password": PASSWORD
                }),
            ),
            peer,
        )
        .await;
    assert_eq!(restored.status, StatusCode::ACCEPTED);
}

fn media_request(blog: &str, token: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(format!("/blogs/{blog}/media"))
        .header("authorization", common::bearer(token))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn upload_ip_budget_is_shared_by_accounts_and_includes_a_daily_limit() {
    let app = TestApp::new().await;
    let peer: IpAddr = "192.0.2.60".parse().unwrap();
    let mut accounts = Vec::new();
    for index in 0..3 {
        let token = register_token(&app, &format!("uploader-{index}@example.com"), PASSWORD).await;
        let blog = create_blog(&app, &token, "Uploads", &format!("uploads-{index}"))
            .await
            .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        accounts.push((token, blog));
    }
    for index in 0..200 {
        if index > 0 && index % 20 == 0 {
            let (token, blog) = &accounts[2];
            let blocked = app
                .send_as(media_request(blog, token, vec![0; 65 * 1024]), peer)
                .await;
            blocked.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
            app.clock.advance(60);
        }
        let account = if index < 20 { index / 10 } else { index % 3 };
        let (token, blog) = &accounts[account];
        let response = app
            .send_as(media_request(blog, token, b"not an image".to_vec()), peer)
            .await;
        assert_eq!(
            response.status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{}",
            response.text()
        );
    }
    let (token, blog) = &accounts[2];
    let daily = app
        .send_as(media_request(blog, token, Vec::new()), peer)
        .await;
    daily.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
    assert!(daily.header("retry-after").unwrap().parse::<u64>().unwrap() > 3600);
    let other_ip = app
        .send_as(
            media_request(blog, token, b"not an image".to_vec()),
            "192.0.2.61".parse().unwrap(),
        )
        .await;
    assert_eq!(other_ip.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn upload_user_budget_survives_ip_pat_and_blog_changes_and_counts_failures() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let (cookie, csrf) = common::login_session(&app, EMAIL, PASSWORD).await;
    let other_token = common::create_pat(&app, &cookie, &csrf, true).await;
    let blog = create_blog(&app, &token, "First", "upload-first")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other_blog = create_blog(&app, &token, "Second", "upload-second")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for index in 0..100 {
        if index > 0 && index % 10 == 0 {
            let blocked = app
                .send_as(
                    media_request(&other_blog, &other_token, vec![0; 65 * 1024]),
                    "192.0.2.99".parse().unwrap(),
                )
                .await;
            blocked.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
            app.clock.advance(60);
        }
        let (blog, token) = if index % 2 == 0 {
            (&blog, &token)
        } else {
            (&other_blog, &other_token)
        };
        let peer = format!("192.0.2.{}", 70 + index % 4).parse().unwrap();
        let response = app
            .send_as(media_request(blog, token, b"not an image".to_vec()), peer)
            .await;
        assert_eq!(
            response.status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{}",
            response.text()
        );
    }
    let daily = app
        .send_as(
            media_request(&other_blog, &other_token, Vec::new()),
            "192.0.2.100".parse().unwrap(),
        )
        .await;
    daily.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
    assert!(daily.header("retry-after").unwrap().parse::<u64>().unwrap() > 3600);
    app.clock.advance(86_400);
    let restored = app
        .send_as(
            media_request(&blog, &token, b"not an image".to_vec()),
            "192.0.2.101".parse().unwrap(),
        )
        .await;
    assert_eq!(restored.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn deleting_media_does_not_refund_upload_attempts() {
    let (app, _storage) = TestApp::with_server_storage().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let blog = create_blog(&app, &token, "Churn", "upload-churn")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut image = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([10u8, 20, 30, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut image),
            image::ImageFormat::Png,
        )
        .unwrap();
    for _ in 0..10 {
        let uploaded = app.send(media_request(&blog, &token, image.clone())).await;
        assert_eq!(uploaded.status, StatusCode::CREATED, "{}", uploaded.text());
        let id = uploaded.json()["id"].as_str().unwrap().to_owned();
        let deleted = common::authed(
            &app,
            Method::DELETE,
            &format!("/blogs/{blog}/media/{id}"),
            &token,
        )
        .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    }
    app.send(media_request(&blog, &token, image))
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media")
        .fetch_one(&app.app.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "storage is empty but the upload budget is spent");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_real_uploads_share_user_budget_and_rejected_bodies_are_not_read() {
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (app, _storage) = TestApp::with_configured_storage(|config| {
        config.r2_default = Some(TestApp::test_server_storage());
        // Keep the general concurrency gate out of this test: the user
        // limiter itself must stop the burst, not an unrelated 503.
        config.max_concurrency = 512;
    })
    .await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let (cookie, csrf) = common::login_session(&app, EMAIL, PASSWORD).await;
    let other_token = common::create_pat(&app, &cookie, &csrf, true).await;
    let mut blogs = Vec::new();
    for slug in ["parallel-one", "parallel-two"] {
        blogs.push(
            create_blog(&app, &token, "Parallel", slug).await.json()["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    let mut image = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([10u8, 20, 30, 255]))
        .write_to(
            &mut std::io::Cursor::new(&mut image),
            image::ImageFormat::Png,
        )
        .unwrap();
    let app = Arc::new(app);
    let barrier = Arc::new(tokio::sync::Barrier::new(257));
    let mut workers = tokio::task::JoinSet::new();
    for index in 0..256 {
        let app = app.clone();
        let barrier = barrier.clone();
        let token = if index % 2 == 0 {
            token.clone()
        } else {
            other_token.clone()
        };
        let blog = blogs[index % 2].clone();
        let image = image.clone();
        workers.spawn(async move {
            let peer = format!("192.0.2.{}", index % 128 + 1).parse().unwrap();
            barrier.wait().await;
            app.send_as(media_request(&blog, &token, image), peer).await
        });
    }
    barrier.wait().await;
    let mut created = 0;
    let mut limited = 0;
    while let Some(result) = workers.join_next().await {
        let response = result.unwrap();
        match response.status {
            StatusCode::CREATED => created += 1,
            StatusCode::TOO_MANY_REQUESTS => {
                assert_eq!(response.header("retry-after"), Some("60"));
                limited += 1;
            }
            status => panic!("unexpected burst response {status}: {}", response.text()),
        }
    }
    assert_eq!((created, limited), (10, 246));
    let mut visible = 0;
    for blog in &blogs {
        let listed =
            common::authed(&app, Method::GET, &format!("/blogs/{blog}/media"), &token).await;
        assert_eq!(listed.status, StatusCode::OK);
        visible += listed.json()["total"].as_u64().unwrap();
    }
    assert_eq!(visible, 10, "only the ten admitted requests created media");

    let reads = Arc::new(AtomicUsize::new(0));
    let body_reads = reads.clone();
    let body = Body::new(
        http_body_util::Full::new(axum::body::Bytes::from_static(b"never read")).map_frame(
            move |frame| {
                body_reads.fetch_add(1, Ordering::SeqCst);
                frame
            },
        ),
    );
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/blogs/{}/media", blogs[0]))
        .header("authorization", common::bearer(&other_token))
        .body(body)
        .unwrap();
    let blocked = app.send_as(request, "203.0.113.5".parse().unwrap()).await;
    blocked.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        0,
        "rejection must precede body buffering"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_signups_with_rotating_ipv6_and_spoofed_headers_share_one_budget() {
    use std::sync::Arc;

    let app = Arc::new(TestApp::build(|config| config.max_concurrency = 256).await);
    let barrier = Arc::new(tokio::sync::Barrier::new(129));
    let mut workers = tokio::task::JoinSet::new();
    for index in 0..128 {
        let app = app.clone();
        let barrier = barrier.clone();
        workers.spawn(async move {
            let request = with_header(
                json_request(
                    Method::POST,
                    "/auth/register",
                    json!({
                        "email": format!("signup-burst-{index}@example.com"), "password": PASSWORD
                    }),
                ),
                "x-forwarded-for",
                &format!("192.0.2.{}", index + 1),
            );
            let peer = format!("2001:db8:1:2::{:x}", index + 1).parse().unwrap();
            barrier.wait().await;
            app.send_as(request, peer).await
        });
    }
    barrier.wait().await;
    let mut accepted = 0;
    let mut rejected = 0;
    while let Some(result) = workers.join_next().await {
        let response = result.unwrap();
        match response.status {
            StatusCode::ACCEPTED => accepted += 1,
            StatusCode::TOO_MANY_REQUESTS => {
                assert_eq!(response.header("retry-after"), Some("3600"));
                rejected += 1;
            }
            status => panic!("unexpected signup response {status}: {}", response.text()),
        }
    }
    assert_eq!((accepted, rejected), (3, 125));
    let other_network = app
        .send_as(
            json_request(
                Method::POST,
                "/auth/register",
                json!({
                    "email": "signup-other-network@example.com", "password": PASSWORD
                }),
            ),
            "2001:db8:1:3::1".parse().unwrap(),
        )
        .await;
    assert_eq!(
        other_network.status,
        StatusCode::ACCEPTED,
        "a different /64 is independent"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_accounts_cannot_overshoot_the_shared_upload_ip_budget() {
    use std::sync::Arc;

    let app = TestApp::new().await;
    let mut accounts = Vec::new();
    for index in 0..3 {
        let token = register_token(
            &app,
            &format!("parallel-account-{index}@example.com"),
            PASSWORD,
        )
        .await;
        let blog = create_blog(&app, &token, "Shared IP", &format!("parallel-ip-{index}"))
            .await
            .json()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        accounts.push((token, blog));
    }
    let app = Arc::new(app);
    let barrier = Arc::new(tokio::sync::Barrier::new(31));
    let mut workers = tokio::task::JoinSet::new();
    for index in 0..30 {
        let app = app.clone();
        let barrier = barrier.clone();
        let (token, blog) = accounts[index % 3].clone();
        workers.spawn(async move {
            barrier.wait().await;
            app.send_as(
                media_request(&blog, &token, b"not an image".to_vec()),
                "192.0.2.220".parse().unwrap(),
            )
            .await
        });
    }
    barrier.wait().await;
    let mut admitted = 0;
    let mut limited = 0;
    while let Some(result) = workers.join_next().await {
        let response = result.unwrap();
        match response.status {
            StatusCode::UNSUPPORTED_MEDIA_TYPE => admitted += 1,
            StatusCode::TOO_MANY_REQUESTS => {
                assert_eq!(response.header("retry-after"), Some("60"));
                limited += 1;
            }
            status => panic!(
                "unexpected shared-IP response {status}: {}",
                response.text()
            ),
        }
    }
    assert_eq!((admitted, limited), (20, 10));
}

async fn login_response(app: &TestApp) -> common::TestResponse {
    app.send(json_request(
        Method::POST,
        "/auth/login",
        json!({ "email": EMAIL, "password": PASSWORD }),
    ))
    .await
}

#[tokio::test]
async fn auth_rate_limit_returns_429_with_retry_after() {
    let app = TestApp::with_limits(1000, 1).await;
    let peer: IpAddr = "192.0.2.10".parse().unwrap();

    let first = app
        .send_as(
            json_request(
                Method::POST,
                "/auth/login",
                json!({ "email": EMAIL, "password": PASSWORD }),
            ),
            peer,
        )
        .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);

    let second = app
        .send_as(
            json_request(
                Method::POST,
                "/auth/login",
                json!({ "email": EMAIL, "password": PASSWORD }),
            ),
            peer,
        )
        .await;
    second.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
    assert!(second.header("retry-after").is_some());
}

#[tokio::test]
async fn general_rate_limit_applies_to_public_reads() {
    let app = TestApp::with_limits(1, 1000).await;
    let peer: IpAddr = "192.0.2.11".parse().unwrap();

    assert_eq!(
        app.send_as(empty(Method::GET, "/public/blogs/none"), peer)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    app.send_as(empty(Method::GET, "/public/blogs/none"), peer)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
}

#[tokio::test]
async fn forwarded_headers_are_only_trusted_from_a_configured_proxy() {
    let app = TestApp::with_limits(1000, 1).await;
    let trusted_peer: IpAddr = "10.0.0.9".parse().unwrap();

    let first = app
        .send_as(
            with_header(
                json_request(
                    Method::POST,
                    "/auth/login",
                    json!({ "email": EMAIL, "password": PASSWORD }),
                ),
                "x-forwarded-for",
                "198.51.100.1",
            ),
            trusted_peer,
        )
        .await;
    assert_eq!(first.status, StatusCode::UNAUTHORIZED);

    let second = app
        .send_as(
            with_header(
                json_request(
                    Method::POST,
                    "/auth/login",
                    json!({ "email": EMAIL, "password": PASSWORD }),
                ),
                "x-forwarded-for",
                "198.51.100.2",
            ),
            trusted_peer,
        )
        .await;
    assert_eq!(
        second.status,
        StatusCode::UNAUTHORIZED,
        "different forwarded client"
    );

    let third = app
        .send_as(
            with_header(
                json_request(
                    Method::POST,
                    "/auth/login",
                    json!({ "email": EMAIL, "password": PASSWORD }),
                ),
                "x-forwarded-for",
                "198.51.100.2",
            ),
            trusted_peer,
        )
        .await;
    third.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");

    let untrusted_peer: IpAddr = "203.0.113.9".parse().unwrap();
    let fourth = app
        .send_as(
            with_header(
                json_request(
                    Method::POST,
                    "/auth/login",
                    json!({ "email": EMAIL, "password": PASSWORD }),
                ),
                "x-forwarded-for",
                "198.51.100.3",
            ),
            untrusted_peer,
        )
        .await;
    assert_eq!(fourth.status, StatusCode::UNAUTHORIZED);

    let fifth = app
        .send_as(
            with_header(
                json_request(
                    Method::POST,
                    "/auth/login",
                    json!({ "email": EMAIL, "password": PASSWORD }),
                ),
                "x-forwarded-for",
                "198.51.100.4",
            ),
            untrusted_peer,
        )
        .await;
    fifth.assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
}

#[tokio::test]
async fn full_authenticated_write_read_round_trip() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Round Trip", "round-trip")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "round-trip", "title": "Round Trip", "content_markdown": "# trip", "status": "published" }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED);

    let listed = app
        .send(empty(Method::GET, "/public/blogs/round-trip/posts"))
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json()["total"], 1);
    assert_eq!(listed.json()["items"][0]["slug"], "round-trip");
}
