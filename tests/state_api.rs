mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::json;

use common::{
    EMAIL, NOW, PASSWORD, TestApp, bearer, create_state, empty, json_request, register_token,
};

fn authed_get(path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .header("authorization", bearer(token))
        .body(Body::empty())
        .unwrap()
}

fn authed_delete(path: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method(Method::DELETE)
        .uri(path)
        .header("authorization", bearer(token))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn full_state_flow_uses_uuidv7_and_seconds() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let created = create_state(
        &app,
        &token,
        "  hello <script>alert(1)</script>  ",
        NOW + 3600,
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let body = created.json();
    let state_id = uuid::Uuid::parse_str(body["state_id"].as_str().unwrap()).unwrap();
    let user_id = uuid::Uuid::parse_str(body["user_id"].as_str().unwrap()).unwrap();
    assert_eq!(state_id.get_version_num(), 7);
    assert_eq!(user_id.get_version_num(), 7);
    assert_eq!(body["text"], "hello <script>alert(1)</script>");
    assert_eq!(body["created_at"], NOW);
    assert_eq!(body["expires_at"], NOW + 3600);

    let latest = app
        .send(empty(Method::GET, "/state/latest?email=USER%40EXAMPLE.COM"))
        .await;
    assert_eq!(latest.status, StatusCode::OK, "{}", latest.text());
    assert_eq!(latest.json()["state_id"], body["state_id"]);
    assert_eq!(latest.json()["email"], EMAIL);

    let listed = app
        .send(empty(
            Method::GET,
            &format!("/state?email={EMAIL}&page=1&size=20"),
        ))
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text());
    assert_eq!(listed.json()["total"], 1);
    assert_eq!(listed.json()["items"][0]["state_id"], body["state_id"]);

    let deleted = app
        .send(authed_delete(&format!("/state/{state_id}"), &token))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert!(deleted.body.is_empty());

    let gone = app
        .send(authed_delete(&format!("/state/{state_id}"), &token))
        .await;
    gone.assert_error(StatusCode::NOT_FOUND, "state not found");
}

#[tokio::test]
async fn list_requires_email_and_paginates_newest_first() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    for index in 0..3 {
        let created = create_state(&app, &token, &format!("state {index}"), NOW + 3600).await;
        assert_eq!(created.status, StatusCode::CREATED);
        app.clock.advance(1);
    }

    let first = app
        .send(empty(
            Method::GET,
            &format!("/state?email={EMAIL}&page=1&size=2"),
        ))
        .await;
    assert_eq!(first.json()["total"], 3);
    assert_eq!(first.json()["page"], 1);
    assert_eq!(first.json()["size"], 2);
    assert_eq!(first.json()["items"][0]["text"], "state 2");
    assert_eq!(first.json()["items"][1]["text"], "state 1");

    let second = app
        .send(empty(
            Method::GET,
            &format!("/state?email={EMAIL}&page=2&size=2"),
        ))
        .await;
    assert_eq!(second.json()["items"].as_array().unwrap().len(), 1);
    assert_eq!(second.json()["items"][0]["text"], "state 0");

    app.send(empty(Method::GET, "/state"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "missing email");

    let unknown = app
        .send(empty(Method::GET, "/state?email=missing@example.com"))
        .await;
    assert_eq!(unknown.status, StatusCode::OK);
    assert_eq!(unknown.json()["items"], json!([]));
    assert_eq!(unknown.json()["total"], 0);
}

#[tokio::test]
async fn pagination_rejects_malformed_values_and_caps_size() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    create_state(&app, &token, "one", NOW + 3600).await;

    for query in [
        "page=abc", "page=0", "page=-1", "size=abc", "size=0", "size=-1",
    ] {
        app.send(empty(Method::GET, &format!("/state?email={EMAIL}&{query}")))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }

    let capped = app
        .send(empty(
            Method::GET,
            &format!("/state?email={EMAIL}&size=500"),
        ))
        .await;
    assert_eq!(capped.json()["size"], 100);
}

#[tokio::test]
async fn latest_is_global_unless_email_scopes_it() {
    let app = TestApp::new().await;
    let a = register_token(&app, EMAIL, PASSWORD).await;
    create_state(&app, &a, "a", NOW + 3600).await;
    app.clock.advance(1);
    let b = register_token(&app, "other@example.com", PASSWORD).await;
    create_state(&app, &b, "b", NOW + 3600).await;

    let global = app.send(empty(Method::GET, "/state/latest")).await;
    assert_eq!(global.status, StatusCode::OK, "{}", global.text());
    assert_eq!(global.json()["text"], "b");
    assert_eq!(global.json()["email"], "other@example.com");

    let scoped = app
        .send(empty(Method::GET, "/state/latest?email=USER%40EXAMPLE.COM"))
        .await;
    assert_eq!(scoped.json()["text"], "a");

    app.send(empty(
        Method::GET,
        "/state/latest?email=missing@example.com",
    ))
    .await
    .assert_error(StatusCode::NOT_FOUND, "state not found");
}

#[tokio::test]
async fn latest_returns_not_found_on_a_fresh_database() {
    let app = TestApp::new().await;
    app.send(empty(Method::GET, "/state/latest"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "state not found");
}

#[tokio::test]
async fn expired_states_remain_readable_and_expiry_is_metadata() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let created = create_state(&app, &token, "still visible", NOW + 10).await;
    assert_eq!(created.status, StatusCode::CREATED);
    app.clock.advance(20);

    let latest = app
        .send(empty(Method::GET, &format!("/state/latest?email={EMAIL}")))
        .await;
    assert_eq!(latest.status, StatusCode::OK);
    assert_eq!(latest.json()["text"], "still visible");
}

#[tokio::test]
async fn create_validation_covers_text_and_expiry_boundaries() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;

    for text in ["", "   ", "\n\t"] {
        create_state(&app, &token, text, NOW + 3600)
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }
    create_state(&app, &token, &"a".repeat(4097), NOW + 3600)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_state(&app, &token, &"a".repeat(4096), NOW + 3600)
        .await
        .status_check(StatusCode::CREATED);

    create_state(&app, &token, "past", NOW)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_state(&app, &token, "too far", NOW + 30 * 24 * 3600 + 1)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
}

#[tokio::test]
async fn delete_is_owner_scoped_and_rejects_invalid_ids() {
    let app = TestApp::new().await;
    let owner = register_token(&app, EMAIL, PASSWORD).await;
    let other = register_token(&app, "other@example.com", PASSWORD).await;
    let created = create_state(&app, &owner, "mine", NOW + 3600).await;
    let state_id = created.json()["state_id"].as_str().unwrap().to_owned();

    app.send(authed_delete(&format!("/state/{state_id}"), &other))
        .await
        .assert_error(StatusCode::NOT_FOUND, "state not found");
    app.send(authed_delete(&format!("/state/{state_id}"), &owner))
        .await
        .status_check(StatusCode::NO_CONTENT);

    app.send(authed_delete("/state/not-a-uuid", &owner))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    let v4 = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    app.send(authed_delete(&format!("/state/{v4}"), &owner))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
}

#[tokio::test]
async fn create_requires_auth_and_strict_json() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;

    app.send(json_request(
        Method::POST,
        "/state",
        json!({ "text": "x", "expires_at": NOW + 3600 }),
    ))
    .await
    .assert_error(StatusCode::UNAUTHORIZED, "missing token");

    let unknown = Request::builder()
        .method(Method::POST)
        .uri("/state")
        .header("authorization", bearer(&token))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "text": "x", "expires_at": NOW + 3600, "user_id": "spoof" }).to_string(),
        ))
        .unwrap();
    app.send(unknown)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid body");

    let trailing = Request::builder()
        .method(Method::POST)
        .uri("/state")
        .header("authorization", bearer(&token))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            "{{\"text\":\"x\",\"expires_at\":{}}}{{}}",
            NOW + 3600
        )))
        .unwrap();
    app.send(trailing)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid body");

    let oversized = Request::builder()
        .method(Method::POST)
        .uri("/state")
        .header("authorization", bearer(&token))
        .header("content-type", "application/json")
        .body(Body::from("x".repeat(33 * 1024)))
        .unwrap();
    app.send(oversized)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
}

#[tokio::test]
async fn missing_email_on_list_does_not_enumerate_or_leak() {
    let app = TestApp::new().await;
    app.send(authed_get("/state", "ignored"))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "missing email");
}
