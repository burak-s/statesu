mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::json;

use common::{
    EMAIL, NOW, PASSWORD, TestApp, bearer, empty, json_request, login, register, register_token,
};

#[tokio::test]
async fn register_returns_uuidv7_identity_and_bearer_token() {
    let app = TestApp::new().await;
    let response = register(&app, "  Visitor@Example.com ", PASSWORD).await;
    assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
    assert!(
        response.header("set-cookie").is_none(),
        "bearer-only backend"
    );

    let body = response.json();
    let id = body["id"].as_str().expect("id");
    let parsed = uuid::Uuid::parse_str(id).expect("uuid");
    assert_eq!(parsed.get_version_num(), 7);
    assert_eq!(body["email"], "visitor@example.com");
    assert!(
        body["token"]
            .as_str()
            .is_some_and(|token| !token.is_empty())
    );
    assert_eq!(body["expires_at"], NOW + 1800);
}

#[tokio::test]
async fn duplicate_registration_is_conflict_after_normalization() {
    let app = TestApp::new().await;
    assert_eq!(
        register(&app, EMAIL, PASSWORD).await.status,
        StatusCode::CREATED
    );
    let duplicate = register(&app, " USER@EXAMPLE.COM ", PASSWORD).await;
    duplicate.assert_error(StatusCode::CONFLICT, "user already exists");
}

#[tokio::test]
async fn invalid_email_and_password_are_distinct_bad_requests() {
    let app = TestApp::new().await;
    register(&app, "not-an-email", PASSWORD)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid email");
    register(&app, EMAIL, "short1").await.assert_error(
        StatusCode::BAD_REQUEST,
        "password must be 8-72 chars with at least one letter and one digit",
    );
}

#[tokio::test]
async fn login_and_me_use_the_same_identity() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    app.clock.advance(1);
    let response = login(&app, "USER@example.com", PASSWORD).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    let login_token = response.json()["token"].as_str().expect("token").to_owned();
    assert_ne!(login_token, token);

    let request = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("authorization", bearer(&login_token))
        .body(Body::empty())
        .unwrap();
    let me = app.send(request).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    assert_eq!(me.json()["email"], EMAIL);
    assert_eq!(me.json()["id"], response.json()["id"]);
}

#[tokio::test]
async fn unknown_email_and_wrong_password_are_generic() {
    let app = TestApp::new().await;
    register(&app, EMAIL, PASSWORD).await;
    login(&app, EMAIL, "wrong password 9")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid credentials");
    login(&app, "missing@example.com", PASSWORD)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid credentials");
    login(&app, "", "")
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid credentials");
}

#[tokio::test]
async fn protected_routes_reject_missing_invalid_expired_and_cookie_tokens() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;

    app.send(empty(Method::GET, "/auth/me"))
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "missing token");

    let invalid = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("authorization", "Bearer not-a-token")
        .body(Body::empty())
        .unwrap();
    app.send(invalid)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");

    let cookie = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("cookie", format!("token={token}"))
        .body(Body::empty())
        .unwrap();
    app.send(cookie)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "missing token");

    app.clock.advance(1801);
    let expired = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("authorization", bearer(&token))
        .body(Body::empty())
        .unwrap();
    app.send(expired)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "token expired");
}

#[tokio::test]
async fn logout_clears_legacy_cookie_without_claiming_revocation() {
    let app = TestApp::new().await;
    let response = app.send(empty(Method::POST, "/auth/logout")).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let cookie = response.header("set-cookie").expect("clearing cookie");
    assert!(cookie.contains("token="));
    assert!(cookie.contains("Max-Age=0"));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("Secure"));
    assert!(response.body.is_empty());
}

#[tokio::test]
async fn json_contract_rejects_unknown_fields_missing_content_type_and_oversized_bodies() {
    let app = TestApp::new().await;
    let unknown = app
        .send(json_request(
            Method::POST,
            "/auth/register",
            json!({ "email": EMAIL, "password": PASSWORD, "admin": true }),
        ))
        .await;
    unknown.assert_error(StatusCode::BAD_REQUEST, "invalid body");

    let missing_type = Request::builder()
        .method(Method::POST)
        .uri("/auth/register")
        .body(Body::from(
            json!({ "email": EMAIL, "password": PASSWORD }).to_string(),
        ))
        .unwrap();
    app.send(missing_type)
        .await
        .assert_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported media type");

    let oversized = Request::builder()
        .method(Method::POST)
        .uri("/auth/register")
        .header("content-type", "application/json")
        .body(Body::from("x".repeat(17 * 1024)))
        .unwrap();
    app.send(oversized)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
}
