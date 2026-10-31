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
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
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
        Some("Authorization, Content-Type")
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
