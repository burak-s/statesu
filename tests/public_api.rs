mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::json;

use common::{
    EMAIL, PASSWORD, TestApp, create_blog, create_post, patch_post, register_token, set_origins,
};

const SITE_ORIGIN: &str = "https://site.example";

async fn seeded_blog_with_published_post(app: &TestApp) {
    let token = register_token(app, EMAIL, PASSWORD).await;
    let blog_id = create_blog(app, &token, "Public Blog", "public-blog")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    set_origins(app, &token, &blog_id, json!([SITE_ORIGIN])).await;
    create_post(
        app,
        &token,
        &blog_id,
        json!({
            "slug": "public",
            "title": "Public Post",
            "content_markdown": "# Public\n",
            "status": "published"
        }),
    )
    .await
    .status_check(axum::http::StatusCode::CREATED);
}

fn request_with_origin(method: Method, path: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header("origin", origin)
        .body(Body::empty())
        .unwrap()
}

fn preflight(path: &str, origin: &str) -> Request<Body> {
    Request::builder()
        .method(Method::OPTIONS)
        .uri(path)
        .header("origin", origin)
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn public_reads_only_show_published_undeleted_posts() {
    let app = TestApp::new().await;
    let token = register_token(&app, EMAIL, PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "published", "title": "Published", "content_markdown": "one", "status": "published" }),
    )
    .await
    .status_check(StatusCode::CREATED);
    create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "draft", "title": "Draft", "content_markdown": "two" }),
    )
    .await
    .status_check(StatusCode::CREATED);
    let withdrawn = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "withdrawn", "title": "Withdrawn", "content_markdown": "three", "status": "published" }),
    )
    .await
    .json()["id"]
    .as_str()
    .unwrap()
    .to_owned();
    patch_post(
        &app,
        &token,
        &blog_id,
        &withdrawn,
        json!({ "status": "draft" }),
    )
    .await;
    let deleted = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "deleted", "title": "Deleted", "content_markdown": "four", "status": "published" }),
    )
    .await
    .json()["id"]
    .as_str()
    .unwrap()
    .to_owned();
    common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{blog_id}/posts/{deleted}"),
        &token,
    )
    .await;

    let listed = app
        .send(common::empty(Method::GET, "/public/blogs/blog/posts"))
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.text());
    let body = listed.json();
    assert_eq!(body["total"], 1);
    assert_eq!(body["items"][0]["slug"], "published");
    assert!(body["items"][0].get("status").is_none());
    assert!(body["items"][0].get("author_id").is_none());

    let post = app
        .send(common::empty(
            Method::GET,
            "/public/blogs/blog/posts/published",
        ))
        .await;
    assert_eq!(post.status, StatusCode::OK);
    let body = post.json();
    assert_eq!(body["title"], "Published");
    assert_eq!(body["content_markdown"], "one");
    assert!(body.get("status").is_none());

    // Drafts, withdrawn posts, deleted posts, unknown slugs are all 404.
    for slug in ["draft", "withdrawn", "deleted", "missing"] {
        app.send(common::empty(
            Method::GET,
            &format!("/public/blogs/blog/posts/{slug}"),
        ))
        .await
        .assert_error(StatusCode::NOT_FOUND, "post not found");
    }

    // Drafts do not appear in supports-page listings for other pages either.
    let page_two = app
        .send(common::empty(
            Method::GET,
            "/public/blogs/blog/posts?page=2",
        ))
        .await;
    assert_eq!(page_two.json()["items"], json!([]));

    // Unknown blog and management-only data.
    app.send(common::empty(Method::GET, "/public/blogs/unknown"))
        .await
        .assert_error(StatusCode::NOT_FOUND, "blog not found");
}

#[tokio::test]
async fn public_blog_profile_is_safe_for_delivery() {
    let app = TestApp::new().await;
    seeded_blog_with_published_post(&app).await;
    let profile = app
        .send(common::empty(Method::GET, "/public/blogs/public-blog"))
        .await;
    assert_eq!(profile.status, StatusCode::OK);
    let body = profile.json();
    assert_eq!(body["name"], "Public Blog");
    assert!(body.get("owner_id").is_none());
    assert!(body.get("origins").is_none());
}

#[tokio::test]
async fn delivery_cors_matches_exact_blog_origins_only() {
    let app = TestApp::new().await;
    let owner = register_token(&app, EMAIL, PASSWORD).await;
    let first_blog = create_blog(&app, &owner, "First", "first").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    set_origins(&app, &owner, &first_blog, json!([SITE_ORIGIN])).await;
    let second_blog = create_blog(&app, &owner, "Second", "second").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    set_origins(&app, &owner, &second_blog, json!(["https://other.example"])).await;

    // Exact origin match wins.
    let allowed = app
        .send(request_with_origin(
            Method::GET,
            "/public/blogs/first",
            SITE_ORIGIN,
        ))
        .await;
    assert_eq!(allowed.status, StatusCode::OK);
    assert_eq!(
        allowed.header("access-control-allow-origin"),
        Some(SITE_ORIGIN)
    );
    assert!(allowed.header("access-control-allow-credentials").is_none());
    assert!(
        allowed
            .header("vary")
            .is_some_and(|value| value.contains("Origin"))
    );

    // A different origin gets no ACAO, even for a valid blog.
    let denied = app
        .send(request_with_origin(
            Method::GET,
            "/public/blogs/first",
            "https://other.example",
        ))
        .await;
    assert_eq!(denied.status, StatusCode::OK);
    assert!(denied.header("access-control-allow-origin").is_none());

    // An origin recorded for another blog grants nothing here.
    let cross = app
        .send(request_with_origin(
            Method::GET,
            "/public/blogs/second",
            SITE_ORIGIN,
        ))
        .await;
    assert_eq!(cross.status, StatusCode::OK);
    assert!(cross.header("access-control-allow-origin").is_none());

    // No Origin header means a plain cross-protocol response without CORS.
    let plain = app
        .send(common::empty(Method::GET, "/public/blogs/first"))
        .await;
    assert!(plain.header("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn delivery_preflights_are_answered_per_blog() {
    let app = TestApp::new().await;
    seeded_blog_with_published_post(&app).await;

    let allowed = app
        .send(preflight(
            "/public/blogs/public-blog/posts/public",
            SITE_ORIGIN,
        ))
        .await;
    assert_eq!(allowed.status, StatusCode::NO_CONTENT);
    assert_eq!(
        allowed.header("access-control-allow-origin"),
        Some(SITE_ORIGIN)
    );
    assert_eq!(
        allowed.header("access-control-allow-methods"),
        Some("GET, HEAD")
    );
    assert!(allowed.header("access-control-allow-credentials").is_none());

    let denied = app
        .send(preflight(
            "/public/blogs/public-blog/posts/public",
            "https://evil.example",
        ))
        .await;
    assert_eq!(denied.status, StatusCode::NO_CONTENT);
    assert!(denied.header("access-control-allow-origin").is_none());

    // Restricting management preflights uses the configured allowlist.
    let management = Request::builder()
        .method(Method::OPTIONS)
        .uri("/blogs")
        .header("origin", common::ALLOWED_ORIGIN)
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();
    let answer = app.send(management).await;
    assert_eq!(answer.status, StatusCode::NO_CONTENT);
    assert_eq!(
        answer.header("access-control-allow-origin"),
        Some(common::ALLOWED_ORIGIN)
    );
}

#[tokio::test]
async fn public_reads_use_the_general_rate_limiter() {
    let app = TestApp::with_limits(1, 5_000).await;
    let peer: std::net::IpAddr = "192.0.2.10".parse().unwrap();

    assert_eq!(
        app.send_as(common::empty(Method::GET, "/public/blogs/none"), peer)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    app.send_as(common::empty(Method::GET, "/public/blogs/none"), peer)
        .await
        .assert_error(StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded");
}

#[tokio::test]
async fn public_responses_are_cacheable_but_nosniffed() {
    let app = TestApp::new().await;
    seeded_blog_with_published_post(&app).await;
    let response = app
        .send(common::empty(Method::GET, "/public/blogs/public-blog"))
        .await;
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert!(response.header("cache-control").is_none());
}
