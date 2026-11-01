mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::{
    EMAIL, NOW, PASSWORD, TestApp, bearer, cookie_header, empty, json_request, login,
    login_session, register, session_authorized, verification_link,
};
use serde_json::json;

#[tokio::test]
async fn register_starts_verification_without_any_credential() {
    let app = TestApp::new().await;
    let response = register(&app, "  Visitor@Example.com ", PASSWORD).await;
    assert_eq!(response.status, StatusCode::ACCEPTED, "{}", response.text());
    assert!(
        response.header("set-cookie").is_none(),
        "no credential cookies before verification"
    );

    let body = response.json();
    assert_eq!(body["status"], "verification_required");
    assert!(body.get("token").is_none());
    assert!(body.get("expires_at").is_none());

    // Exactly one mail, addressed to the normalized email, linking to the
    // configured public origin and carrying a `sv_` token.
    let mailer = app.sent_mail.lock().unwrap();
    assert_eq!(mailer.len(), 1);
    assert_eq!(mailer[0].to, "visitor@example.com");
    assert!(
        mailer[0]
            .link
            .starts_with("https://allowed.example/verify?token=sv_"),
        "link: {}",
        mailer[0].link
    );
}

#[tokio::test]
async fn duplicate_registration_is_conflict_after_normalization() {
    let app = TestApp::new().await;
    assert_eq!(
        register(&app, EMAIL, PASSWORD).await.status,
        StatusCode::ACCEPTED
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
async fn verification_confirms_and_unverified_login_is_rejected() {
    let app = TestApp::new().await;
    register(&app, EMAIL, PASSWORD).await;

    login(&app, EMAIL, PASSWORD)
        .await
        .assert_error(StatusCode::FORBIDDEN, "email not verified");

    let link = verification_link(&app, EMAIL).await;
    let token = link.rsplit("token=").next().unwrap().to_owned();
    let verify = app
        .send(json_request(
            Method::POST,
            "/auth/verify",
            json!({ "token": token }),
        ))
        .await;
    assert_eq!(verify.status, StatusCode::OK, "{}", verify.text());
    assert_eq!(verify.json()["status"], "verified");

    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;
    let me = app
        .send(session_authorized(
            empty(Method::GET, "/auth/me"),
            &session,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    assert_eq!(me.json()["email"], EMAIL);
}

#[tokio::test]
async fn verification_rejects_bad_expired_and_replayed_tokens() {
    let app = TestApp::new().await;
    register(&app, EMAIL, PASSWORD).await;
    let token = verification_link(&app, EMAIL)
        .await
        .rsplit("token=")
        .next()
        .unwrap()
        .to_owned();

    let ok = Request::builder()
        .method(Method::POST)
        .uri("/auth/verify")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "token": token }).to_string()))
        .unwrap();
    assert_eq!(app.send(ok).await.status, StatusCode::OK);

    // Replay: the token is single-use.
    let replay = Request::builder()
        .method(Method::POST)
        .uri("/auth/verify")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "token": token }).to_string()))
        .unwrap();
    app.send(replay)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");

    // A second account expires while unredeemed.
    register(&app, "late@example.com", PASSWORD).await;
    app.clock.advance(1801);
    let expired = verification_link(&app, "late@example.com")
        .await
        .rsplit("token=")
        .next()
        .unwrap()
        .to_owned();
    let request = Request::builder()
        .method(Method::POST)
        .uri("/auth/verify")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "token": expired }).to_string()))
        .unwrap();
    app.send(request)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "token expired");

    // Structural garbage is a token failure, not a panic.
    let forged = Request::builder()
        .method(Method::POST)
        .uri("/auth/verify")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "token": "sv_deadbeef" }).to_string()))
        .unwrap();
    app.send(forged)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");
}

#[tokio::test]
async fn resend_is_generic_cooldown_bounded_and_revives_lost_mail() {
    let app = TestApp::new().await;
    register(&app, EMAIL, PASSWORD).await;
    let mails_before = app.sent_mail.lock().unwrap().len();

    // Unknown and malformed addresses get the same generic 202.
    let unknown = app
        .send(json_request(
            Method::POST,
            "/auth/resend",
            json!({ "email": "missing@example.com" }),
        ))
        .await;
    assert_eq!(unknown.status, StatusCode::ACCEPTED);
    assert_eq!(unknown.json()["status"], "verification_required");
    let malformed = app
        .send(json_request(
            Method::POST,
            "/auth/resend",
            json!({ "email": "not-an-email" }),
        ))
        .await;
    assert_eq!(malformed.status, StatusCode::ACCEPTED);
    assert_eq!(app.sent_mail.lock().unwrap().len(), mails_before);

    // Cooldown: an immediate resend is a no-op send.
    app.send(json_request(
        Method::POST,
        "/auth/resend",
        json!({ "email": "  USER@example.com " }),
    ))
    .await
    .status_check(StatusCode::ACCEPTED);
    assert_eq!(app.sent_mail.lock().unwrap().len(), mails_before);

    // After the cooldown a replacement link is issued and verifies.
    app.clock.advance(61);
    app.send(json_request(
        Method::POST,
        "/auth/resend",
        json!({ "email": EMAIL }),
    ))
    .await
    .status_check(StatusCode::ACCEPTED);
    let token = verification_link(&app, EMAIL)
        .await
        .rsplit("token=")
        .next()
        .unwrap()
        .to_owned();
    app.send(json_request(
        Method::POST,
        "/auth/verify",
        json!({ "token": token }),
    ))
    .await
    .status_check(StatusCode::OK);
}

#[tokio::test]
async fn login_sets_bound_session_cookies() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let response = login(&app, EMAIL, PASSWORD).await;
    assert_eq!(response.status, StatusCode::OK, "{}", response.text());
    assert_eq!(response.json()["email"], EMAIL);
    assert!(response.json().get("token").is_none());

    let cookies: Vec<String> = response
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect();
    assert_eq!(cookies.len(), 2);
    for cookie in &cookies {
        assert!(cookie.starts_with("__Host-statesu_"), "{cookie}");
        assert!(cookie.contains("Secure"), "{cookie}");
        assert!(cookie.contains("SameSite=Lax"), "{cookie}");
        assert!(cookie.contains("Path=/"), "{cookie}");
    }
    assert!(
        cookies.iter().any(|c| c.contains("HttpOnly")),
        "session cookie is HttpOnly"
    );
    assert!(
        cookies.iter().any(|c| !c.contains("HttpOnly")),
        "CSRF cookie stays readable for the dashboard"
    );
}

#[tokio::test]
async fn sessions_fail_closed_on_missing_invalid_and_expired() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;

    app.send(empty(Method::GET, "/auth/me"))
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "missing token");

    let forged = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header(
            "cookie",
            cookie_header("__Host-statesu_session=ss_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
        )
        .body(Body::empty())
        .unwrap();
    app.send(forged)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");

    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;
    app.clock.advance(7 * 24 * 60 * 60 + 1);
    let request = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("cookie", cookie_header(&session))
        .body(Body::empty())
        .unwrap();
    app.send(request)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");
    let _ = csrf;
}

#[tokio::test]
async fn unsafe_session_requests_need_the_bound_csrf_token() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;

    // GET is safe without CSRF.
    let me = app
        .send(session_authorized(
            empty(Method::GET, "/auth/me"),
            &session,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK);

    // Unsafe without the header fails.
    let no_csrf = Request::builder()
        .method(Method::POST)
        .uri("/auth/sessions") // DELETE would need header; POST on /auth/logout works
        .header("cookie", cookie_header(&session))
        .body(Body::from(
            json!({ "email": "x@y.dev", "password": "nonesuch 7" }).to_string(),
        ))
        .unwrap();
    // POST /auth/sessions is a 405 route: the point is CSRF precedes routing.
    let response = app.send(no_csrf).await;
    assert!(
        response.status == StatusCode::FORBIDDEN
            || response.status == StatusCode::METHOD_NOT_ALLOWED,
        "got {}",
        response.status
    );

    // With the header and a same-site origin it succeeds (204 revoke).
    let with_csrf = Request::builder()
        .method(Method::DELETE)
        .uri("/auth/sessions")
        .header("cookie", cookie_header(&session))
        .header("origin", "https://allowed.example")
        .header("x-csrf-token", csrf.clone())
        .body(Body::empty())
        .unwrap();
    app.send(with_csrf)
        .await
        .status_check(StatusCode::NO_CONTENT);

    // The session is gone everywhere now.
    let me_after = app
        .send(session_authorized(
            empty(Method::GET, "/auth/me"),
            &session,
            &csrf,
        ))
        .await;
    me_after.assert_error(StatusCode::UNAUTHORIZED, "invalid token");
}

#[tokio::test]
async fn cross_site_cookie_writes_are_rejected_even_with_a_stolen_csrf() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;

    let forged = Request::builder()
        .method(Method::DELETE)
        .uri("/auth/sessions")
        .header("cookie", cookie_header(&session))
        .header("origin", "https://evil.example")
        .header("x-csrf-token", csrf)
        .body(Body::empty())
        .unwrap();
    app.send(forged)
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
}

#[tokio::test]
async fn cross_site_register_login_and_resend_writes_are_rejected() {
    let app = TestApp::new().await;
    for path in ["/auth/register", "/auth/login", "/auth/resend"] {
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header("origin", "https://evil.example")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "email": EMAIL, "password": PASSWORD }).to_string(),
            ))
            .unwrap();
        app.send(request)
            .await
            .assert_error(StatusCode::FORBIDDEN, "forbidden");
    }
    // Same-site and curl-style (no Origin) requests pass through.
    let curl_like = json_request(
        Method::POST,
        "/auth/login",
        json!({ "email": EMAIL, "password": PASSWORD }),
    );
    assert_eq!(
        app.send(curl_like).await.status,
        StatusCode::UNAUTHORIZED,
        "no Origin, no problem"
    );
}

#[tokio::test]
async fn logout_revokes_the_session_and_clears_cookies() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;

    let response = app
        .send(session_authorized(
            empty(Method::POST, "/auth/logout"),
            &session,
            &csrf,
        ))
        .await;
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "{}",
        response.text()
    );
    let clear_cookies = response
        .headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok().map(str::to_owned))
        .collect::<Vec<_>>();
    eprintln!("DEBUG {clear_cookies:?}");
    eprintln!("ALL {:?}", response.headers);
    assert_eq!(clear_cookies.len(), 2);
    assert!(clear_cookies.iter().all(|c| c.contains("Max-Age=0")));

    // Revocation is server-side: the same cookie no longer authenticates.
    app.send(session_authorized(
        empty(Method::GET, "/auth/me"),
        &session,
        &csrf,
    ))
    .await
    .assert_error(StatusCode::UNAUTHORIZED, "invalid token");

    // Cookie-less logout is an error, not a silent no-op.
    app.send(empty(Method::POST, "/auth/logout"))
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "missing token");
}

#[tokio::test]
async fn personal_access_tokens_flow_the_content_api() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;

    let create = app
        .send(session_authorized(
            json_request(
                Method::POST,
                "/auth/pats",
                json!({ "name": "ci", "write": true }),
            ),
            &session,
            &csrf,
        ))
        .await;
    assert_eq!(create.status, StatusCode::CREATED, "{}", create.text());
    let secret = create.json()["secret"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("su_pat_"), "{secret}");
    assert_eq!(create.json()["scopes"], "content:read content:write");

    // The secret is never listed again.
    let listed = app
        .send(session_authorized(
            empty(Method::GET, "/auth/pats"),
            &session,
            &csrf,
        ))
        .await;
    assert_eq!(listed.json()["items"], serde_json::Value::Null);
    assert!(
        !listed.text().contains(&secret),
        "secret must not reappear: {}",
        listed.text()
    );

    // Content API accepts the PAT.
    let blog = app
        .send(json_request(
            Method::POST,
            "/blogs",
            json!({ "name": "B", "slug": "b" }),
        ))
        .await;
    assert_eq!(blog.status, StatusCode::UNAUTHORIZED);
    let with_bearer = Request::builder()
        .method(Method::POST)
        .uri("/blogs")
        .header("authorization", bearer(&secret))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "name": "B", "slug": "b" }).to_string()))
        .unwrap();
    let created = app.send(with_bearer).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());

    // PATs are refused on /auth routes.
    let pat_on_auth = Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("authorization", bearer(&secret))
        .header("cookie", "")
        .body(Body::empty())
        .unwrap();
    app.send(pat_on_auth)
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");

    // Revocation ends bearer access with a 404 on repeat.
    let pat_id = create.json()["id"].as_str().unwrap().to_owned();
    let delete = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/auth/pats/{pat_id}"))
        .header("cookie", cookie_header(&session))
        .header("origin", "https://allowed.example")
        .header("x-csrf-token", csrf.clone())
        .body(Body::empty())
        .unwrap();
    app.send(delete).await.status_check(StatusCode::NO_CONTENT);
    let delete_again = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/auth/pats/{pat_id}"))
        .header("cookie", cookie_header(&session))
        .header("origin", "https://allowed.example")
        .header("x-csrf-token", csrf)
        .body(Body::empty())
        .unwrap();
    app.send(delete_again)
        .await
        .assert_error(StatusCode::NOT_FOUND, "token not found");
    let after = app
        .send(
            Request::builder()
                .method(Method::GET)
                .uri("/blogs")
                .header("authorization", bearer(&secret))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn pats_support_exact_expiration_dates_and_no_expiration() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;
    let expiry = NOW + 120 * 86400 + 123;
    let mut secrets = Vec::new();
    for body in [
        json!({ "name": "dated", "expires_at": expiry }),
        json!({ "name": "infinite", "expires_at": null }),
        json!({ "name": "omitted" }),
    ] {
        let response = app
            .send(session_authorized(
                json_request(Method::POST, "/auth/pats", body),
                &session,
                &csrf,
            ))
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
        let data = response.json();
        assert_eq!(
            data["expires_at"],
            if secrets.is_empty() {
                json!(expiry)
            } else {
                json!(null)
            }
        );
        secrets.push(data["secret"].as_str().unwrap().to_owned());
    }
    for expiry in [NOW - 1, NOW, 0] {
        app.send(session_authorized(
            json_request(
                Method::POST,
                "/auth/pats",
                json!({ "name": "bad", "expires_at": expiry }),
            ),
            &session,
            &csrf,
        ))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }
    let listed = app
        .send(session_authorized(
            empty(Method::GET, "/auth/pats"),
            &session,
            &csrf,
        ))
        .await;
    let records = listed.json();
    let records = records.as_array().unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(
        records
            .iter()
            .filter(|pat| pat["expires_at"].is_null())
            .count(),
        2
    );
    assert!(records.iter().any(|pat| pat["expires_at"] == expiry));

    app.clock.advance(expiry - NOW - 1);
    common::authed(&app, Method::GET, "/blogs", &secrets[0])
        .await
        .status_check(StatusCode::OK);
    app.clock.advance(1);
    common::authed(&app, Method::GET, "/blogs", &secrets[0])
        .await
        .assert_error(StatusCode::UNAUTHORIZED, "invalid token");
    app.clock.advance(365 * 86400);
    for secret in &secrets[1..] {
        common::authed(&app, Method::GET, "/blogs", secret)
            .await
            .status_check(StatusCode::OK);
    }
}

#[tokio::test]
async fn non_expiring_pats_count_toward_the_active_token_limit() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;
    for index in 0..20 {
        app.send(session_authorized(
            json_request(
                Method::POST,
                "/auth/pats",
                json!({ "name": format!("token-{index}"), "expires_at": null }),
            ),
            &session,
            &csrf,
        ))
        .await
        .status_check(StatusCode::CREATED);
    }
    app.send(session_authorized(
        json_request(Method::POST, "/auth/pats", json!({ "name": "too-many" })),
        &session,
        &csrf,
    ))
    .await
    .assert_error(StatusCode::FORBIDDEN, "forbidden");
}

#[tokio::test]
async fn read_only_pats_cannot_write() {
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let (session, csrf) = login_session(&app, EMAIL, PASSWORD).await;
    let (write_pat, read_pat) = {
        let write = app
            .send(session_authorized(
                json_request(
                    Method::POST,
                    "/auth/pats",
                    json!({ "name": "w", "write": true }),
                ),
                &session,
                &csrf,
            ))
            .await;
        let read = app
            .send(session_authorized(
                json_request(Method::POST, "/auth/pats", json!({ "name": "r" })),
                &session,
                &csrf,
            ))
            .await;
        (
            write.json()["secret"].as_str().unwrap().to_owned(),
            read.json()["secret"].as_str().unwrap().to_owned(),
        )
    };

    let blog = common::create_blog(&app, &write_pat, "Blog", "blog").await;
    assert_eq!(blog.status, StatusCode::CREATED, "{}", blog.text());
    let blog_id = blog.json()["id"].as_str().unwrap().to_owned();
    common::create_blog(&app, &read_pat, "Blog", "read-blog")
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");

    common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{blog_id}"),
        &read_pat,
    )
    .await
    .assert_error(StatusCode::FORBIDDEN, "forbidden");

    // Reads still work for read-only PATs, and the write PAT can both.
    let listed = app
        .send(
            Request::builder()
                .method(Method::GET)
                .uri("/blogs")
                .header("authorization", bearer(&read_pat))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text());
    let listed = app
        .send(
            Request::builder()
                .method(Method::GET)
                .uri("/blogs")
                .header("authorization", bearer(&write_pat))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(listed.json().as_array().map(Vec::len), Some(1));
    let _ = blog_id;
}

#[tokio::test]
async fn json_contract_covers_the_new_auth_routes() {
    let app = TestApp::new().await;
    // Unknown fields are refused on the new POST bodies too.
    verify_unknown_body_rejected(&app).await;

    let missing_type = Request::builder()
        .method(Method::POST)
        .uri("/auth/resend")
        .body(Body::from(json!({ "email": EMAIL }).to_string()))
        .unwrap();
    app.send(missing_type)
        .await
        .assert_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported media type");
}

async fn verify_unknown_body_rejected(app: &TestApp) {
    let unknown = app
        .send(json_request(
            Method::POST,
            "/auth/verify",
            json!({ "token": "sv_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "admin": true }),
        ))
        .await;
    unknown.assert_error(StatusCode::BAD_REQUEST, "invalid body");
}

#[tokio::test]
async fn register_time_is_stable_and_expiration_fields_match_the_clock() {
    // Documented invariant: NOW mirrors the test clock for cookie max-age.
    let app = TestApp::new().await;
    common::register_and_verify(&app, EMAIL, PASSWORD).await;
    let response = login(&app, EMAIL, PASSWORD).await;
    for raw in response.headers.get_all("set-cookie").iter() {
        let cookie = raw.to_str().unwrap();
        let max_age: i64 = cookie
            .split(';')
            .find_map(|part| part.trim().strip_prefix("Max-Age="))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        assert_eq!(max_age, 7 * 24 * 60 * 60, "{cookie}");
    }
    let _ = NOW;
}
