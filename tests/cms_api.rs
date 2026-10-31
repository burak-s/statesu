mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};

use common::{
    TestApp, bearer, create_blog, create_post, json_request, patch_post, register_token,
    set_origins,
};

fn authed_json(method: Method, path: &str, token: &str, body: Value) -> Request<Body> {
    let request = json_request(method, path, body);
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    Request::from_parts(parts, body)
}

#[tokio::test]
async fn blog_create_list_and_conflicts() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;

    let created = create_blog(&app, &token, "  Test Blog  ", "test-blog").await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let body = created.json();
    let id = body["id"].as_str().unwrap();
    assert_eq!(uuid::Uuid::parse_str(id).unwrap().get_version_num(), 7);
    assert_eq!(body["name"], "Test Blog");
    assert_eq!(body["slug"], "test-blog");
    assert_eq!(body["domain"], Value::Null);
    assert_eq!(body["origins"], json!([]));
    assert_eq!(body["created_at"], common::NOW);

    // Slug conflicts are stable 409s.
    create_blog(&app, &token, "Other", "test-blog")
        .await
        .assert_error(StatusCode::CONFLICT, "slug already in use");
    create_blog(&app, &token, "Own", "other-blog").await;

    // Domains are normalized and must be unique across all blogs.
    app.send(authed_json(
        Method::POST,
        "/blogs",
        &token,
        json!({ "name": "Bad", "slug": "bad", "domain": "has space.example" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    let created_domain = app
        .send(authed_json(
            Method::POST,
            "/blogs",
            &token,
            json!({ "name": "With Domain", "slug": "with-domain", "domain": "Example.com" }),
        ))
        .await;
    assert_eq!(created_domain.status, StatusCode::CREATED);
    assert_eq!(created_domain.json()["domain"], "example.com");

    app.send(authed_json(
        Method::POST,
        "/blogs",
        &token,
        json!({ "name": "Clash", "slug": "clash", "domain": "http://example.com/notes" }),
    ))
    .await
    .assert_error(StatusCode::CONFLICT, "domain already claimed");

    // Listing is owner-scoped.
    let list = common::authed(&app, Method::GET, "/blogs", &token).await;
    assert_eq!(list.status, StatusCode::OK);
    let body = list.json();
    let items = body.as_array().unwrap();
    assert_eq!(items.len(), 3, "{}", list.text());

    let other = register_token(&app, "other@example.com", common::PASSWORD).await;
    let foreign = common::authed(&app, Method::GET, "/blogs", &other).await;
    assert_eq!(foreign.json().as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn blog_create_validates_and_requires_auth() {
    let app = TestApp::new().await;
    app.send(json_request(
        Method::POST,
        "/blogs",
        json!({ "name": "x", "slug": "x" }),
    ))
    .await
    .assert_error(StatusCode::UNAUTHORIZED, "missing token");

    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    for slug in [
        "",
        "Not-Valid",
        "-leading",
        "trailing-",
        "a".repeat(64).as_str(),
    ] {
        app.send(authed_json(
            Method::POST,
            "/blogs",
            &token,
            json!({ "name": "Blog", "slug": slug }),
        ))
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }
    app.send(authed_json(
        Method::POST,
        "/blogs",
        &token,
        json!({ "name": "  ", "slug": "fine" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    app.send(authed_json(
        Method::POST,
        "/blogs",
        &token,
        json!({ "name": "Blog", "slug": "fine", "domain": "has space.example" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
}

#[tokio::test]
async fn blog_limit_per_user_is_enforced() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    for index in 0..20 {
        create_blog(&app, &token, "Blog", &format!("blog-{index}"))
            .await
            .status_check(StatusCode::CREATED);
    }
    create_blog(&app, &token, "Blog", "one-too-many")
        .await
        .assert_error(StatusCode::FORBIDDEN, "forbidden");
}

#[tokio::test]
async fn blog_patch_updates_name_and_domain_and_is_owner_scoped() {
    let app = TestApp::new().await;
    let owner = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let stranger = register_token(&app, "other@example.com", common::PASSWORD).await;
    let id = create_blog(&app, &owner, "Mine", "mine").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let foreign = app
        .send(authed_json(
            Method::PATCH,
            &format!("/blogs/{id}"),
            &stranger,
            json!({ "name": "Stolen" }),
        ))
        .await;
    foreign.assert_error(StatusCode::NOT_FOUND, "blog not found");

    let patched = app
        .send(authed_json(
            Method::PATCH,
            &format!("/blogs/{id}"),
            &owner,
            json!({ "name": "Renamed", "domain": "blog.example" }),
        ))
        .await;
    assert_eq!(patched.status, StatusCode::OK);
    let body = patched.json();
    assert_eq!(body["name"], "Renamed");
    assert_eq!(body["domain"], "blog.example");

    // Clearing a domain with an explicit null and invalid values rejected.
    let cleared = app
        .send(authed_json(
            Method::PATCH,
            &format!("/blogs/{id}"),
            &owner,
            json!({ "domain": Value::Null }),
        ))
        .await;
    assert_eq!(cleared.json()["domain"], Value::Null);
    app.send(authed_json(
        Method::PATCH,
        &format!("/blogs/{id}"),
        &owner,
        json!({ "domain": "https://" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    // Unknown and malformed IDs never leak existence.
    app.send(authed_json(
        Method::PATCH,
        "/blogs/00000000-0000-0000-0000-000000000000",
        &owner,
        json!({ "name": "x" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    app.send(authed_json(
        Method::PATCH,
        "/blogs/not-a-uuid",
        &owner,
        json!({ "name": "x" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
}

#[tokio::test]
async fn origins_are_normalized_replaced_and_validated() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let set = set_origins(
        &app,
        &token,
        &id,
        json!([
            "https://Example.com",
            "http://localhost:5173",
            "https://example.com" // duplicate after normalization
        ]),
    )
    .await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.text());
    assert_eq!(
        set.json()["origins"],
        json!(["http://localhost:5173", "https://example.com"])
    );

    // Replace semantics: the second call drops the first list.
    let replaced = set_origins(&app, &token, &id, json!(["https://other.example"])).await;
    assert_eq!(replaced.json()["origins"], json!(["https://other.example"]));

    for bad in [
        "example.com",
        "ftp://example.com",
        "https://example.com/path",
        "",
    ] {
        set_origins(&app, &token, &id, json!([bad]))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }
    set_origins(&app, &token, &id, json!([]))
        .await
        .status_check(StatusCode::OK);
    set_origins(
        &app,
        &token,
        &id,
        json!(["https://x.example", "https://y.example/"]),
    )
    .await;
}

#[tokio::test]
async fn post_lifecycle_covers_draft_publish_withdraw_and_delete() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let created = create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "hello-world",
            "title": "Hello",
            "content_markdown": "# Hello\n"
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let post = created.json();
    assert_eq!(post["status"], "draft");
    assert_eq!(post["published_at"], Value::Null);
    assert_eq!(post["created_at"], common::NOW);
    let post_id = post["id"].as_str().unwrap().to_owned();

    // Drafts are listed for the owner with pagination metadata.
    let listed = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{blog_id}/posts"),
        &token,
    )
    .await;
    assert_eq!(listed.json()["total"], 1);
    assert_eq!(listed.json()["items"][0]["slug"], "hello-world");

    // Publishing records published_at using the clock.
    app.clock.advance(2);
    let published = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "status": "published" }),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.text());
    assert_eq!(published.json()["published_at"], common::NOW + 2);

    // Withdrawing goes back to a draft without a publication timestamp.
    let withdrawn = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "status": "draft" }),
    )
    .await;
    assert_eq!(withdrawn.json()["status"], "draft");
    assert_eq!(withdrawn.json()["published_at"], Value::Null);

    // Management get + edit + delete flow.
    let got = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{blog_id}/posts/{post_id}"),
        &token,
    )
    .await;
    assert_eq!(got.json()["title"], "Hello");

    let edited = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "title": " Hello 2 ", "content_markdown": "# Hello 2" }),
    )
    .await;
    assert_eq!(edited.json()["title"], "Hello 2");
    assert_eq!(edited.json()["content_markdown"], "# Hello 2");

    let deleted = common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{blog_id}/posts/{post_id}"),
        &token,
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{blog_id}/posts/{post_id}"),
        &token,
    )
    .await
    .assert_error(StatusCode::NOT_FOUND, "post not found");

    // The slug stays reserved by the soft-deleted row.
    create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "hello-world", "title": "New", "content_markdown": "x" }),
    )
    .await
    .assert_error(StatusCode::CONFLICT, "slug already in use");
}

#[tokio::test]
async fn post_create_publishes_in_one_step_when_requested() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let published = create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "now",
            "title": "Now",
            "content_markdown": "x",
            "status": "published"
        }),
    )
    .await;
    assert_eq!(published.json()["published_at"], common::NOW);

    create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "bad",
            "title": "Bad",
            "content_markdown": "x",
            "status": "unlisted"
        }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
}

#[tokio::test]
async fn post_rules_strict_json_limit_and_ownership() {
    let app = TestApp::new().await;
    let owner = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let stranger = register_token(&app, "other@example.com", common::PASSWORD).await;
    let blog_id = create_blog(&app, &owner, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Unauthenticated, unknown blog, foreign blog, invalid slug, huge title.
    app.send(json_request(
        Method::POST,
        &format!("/blogs/{blog_id}/posts"),
        json!({ "slug": "x", "title": "x", "content_markdown": "x" }),
    ))
    .await
    .assert_error(StatusCode::UNAUTHORIZED, "missing token");
    create_post(
        &app,
        &stranger,
        &blog_id,
        json!({ "slug": "x", "title": "x", "content_markdown": "x" }),
    )
    .await
    .assert_error(StatusCode::NOT_FOUND, "blog not found");
    app.send(authed_json(
        Method::POST,
        "/blogs/not-a-uuid/posts",
        &owner,
        json!({ "slug": "x", "title": "x", "content_markdown": "x" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_post(
        &app,
        &owner,
        &blog_id,
        json!({ "slug": "Bad Slug", "title": "x", "content_markdown": "x" }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_post(
        &app,
        &owner,
        &blog_id,
        json!({ "slug": "ok", "title": "  ", "content_markdown": "x" }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    // Content beyond the transport limit is cut off deterministically; the
    // field limit itself is enforced before storage.
    create_post(
        &app,
        &owner,
        &blog_id,
        json!({ "slug": "big", "title": "x", "content_markdown": "a".repeat(256 * 1024 + 1) }),
    )
    .await
    .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");

    create_post(
        &app,
        &owner,
        &blog_id,
        json!({ "slug": "big-ok", "title": "x", "content_markdown": "a".repeat(100 * 1024) }),
    )
    .await
    .status_check(StatusCode::CREATED);

    // Strict JSON: unknown fields and trailing data are rejected.
    let spoof = Request::builder()
        .method(Method::POST)
        .uri(format!("/blogs/{blog_id}/posts"))
        .header("authorization", bearer(&owner))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"slug": "s", "title": "t", "content_markdown": "x", "author_id": "me"})
                .to_string(),
        ))
        .unwrap();
    app.send(spoof)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid body");
}

#[tokio::test]
async fn post_pagination_is_bounded_and_ordered() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for index in 0..3 {
        create_post(
            &app,
            &token,
            &blog_id,
            json!({
                "slug": format!("post-{index}"),
                "title": format!("Post {index}"),
                "content_markdown": "x"
            }),
        )
        .await
        .status_check(StatusCode::CREATED);
        app.clock.advance(1);
    }

    let first = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{blog_id}/posts?page=1&size=2"),
        &token,
    )
    .await;
    assert_eq!(first.json()["total"], 3);
    assert_eq!(first.json()["items"][0]["slug"], "post-2");
    assert_eq!(first.json()["items"][1]["slug"], "post-1");

    let second = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{blog_id}/posts?page=2&size=2"),
        &token,
    )
    .await;
    assert_eq!(second.json()["items"].as_array().unwrap().len(), 1);

    common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{blog_id}/posts?page=0"),
        &token,
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    let capped = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{blog_id}/posts?size=500"),
        &token,
    )
    .await;
    assert_eq!(capped.json()["size"], 100);
}

#[tokio::test]
async fn strict_json_body_limit_is_256_kib_for_cms_writes() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let oversized = Request::builder()
        .method(Method::POST)
        .uri("/blogs")
        .header("authorization", bearer(&token))
        .header("content-type", "application/json")
        .body(Body::from("x".repeat(CMS_BODY_BYTES + 1)))
        .unwrap();
    app.send(oversized)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE, "request body too large");
}

const CMS_BODY_BYTES: usize = 256 * 1024;

#[tokio::test]
async fn origins_validation_hardens_hosts_and_ports() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Hostname, IDN punycode, IPv4 and bracketed IPv6 literals with ports.
    let set = set_origins(
        &app,
        &token,
        &id,
        json!([
            "https://xn--bcher-kva.example.com",
            "http://127.0.0.1:3000",
            "http://[::1]:5173",
            "HTTP://EXAMPLE.com"
        ]),
    )
    .await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.text());
    assert_eq!(
        set.json()["origins"],
        json!([
            "http://127.0.0.1:3000",
            "http://[::1]:5173",
            "http://example.com",
            "https://xn--bcher-kva.example.com"
        ])
    );

    for bad in [
        "https://example.com:0",
        "https://example.com:65536",
        "https://example.com:",
        "https://a-.example",
        "https://-a.example",
        "https://a_b.example",
        "https://[::1",
        "https://[not-ip6]",
        "https://a..b",
        "https://user@example.com",
    ] {
        set_origins(&app, &token, &id, json!([bad]))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }
}

#[tokio::test]
async fn content_hardening_normalizes_and_rejects_control_input() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // CRLF normalized to LF, CR alone to LF, BOM stripped.
    let created = create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "normalized",
            "title": "Normalized",
            "content_markdown": "\u{feff}# A\r\nline\rtwo\n"
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    assert_eq!(created.json()["content_markdown"], "# A\nline\ntwo\n");

    // NUL, other C0 controls, DEL and C1 controls are rejected.
    for content in ["x\u{0}y", "x\u{1}y", "x\u{7f}y", "a\u{9f}b"] {
        create_post(
            &app,
            &token,
            &blog_id,
            json!({
                "slug": "bad-content",
                "title": "Bad",
                "content_markdown": content
            }),
        )
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }

    // Titles must be printable, single-line and trimmed-safe.
    create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "bad-title",
            "title": "Bad\u{0}Title",
            "content_markdown": "x"
        }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "bad-title",
            "title": "Bad\nTitle",
            "content_markdown": "x"
        }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "bad-title",
            "title": "Bad\tTitle",
            "content_markdown": "x"
        }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    // Blog names follow the same printable single-line rule.
    create_blog(&app, &token, "Bad\nBlog", "bad-blog")
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    create_blog(&app, &token, "Bad\u{7f}Blog", "bad-blog-2")
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    // The update path enforces the same rules.
    let post_id = created.json()["id"].as_str().unwrap().to_owned();
    patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "ok\u{1}" }),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    let patched = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "# Now\r\nfine" }),
    )
    .await;
    assert_eq!(patched.json()["content_markdown"], "# Now\nfine");
}

#[tokio::test]
async fn domain_validation_rejects_malformed_hosts() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;

    for (spec, domain) in [
        ("exa mple.com", false),
        ("a..b", false),
        ("a-.example", false),
        ("example.com:0", false),
        ("example.com:65536", false),
        ("user@example.com", false),
        ("my_site.example", false),
        ("ftp://example.com", false),
        ("[not-ip6]", false),
        ("xn--bcher-kva.example.com", true),
        ("Example.com:8080", true),
        ("xn--bcher-kva.example", true),
        ("[::1]:5173", true),
    ] {
        let slug = format!(
            "d-{}",
            DOMAIN_SLUG_COUNTER.with(|cell| {
                let value = cell.get() + 1;
                cell.set(value);
                value
            })
        );
        let response = app
            .send(authed_json(
                Method::POST,
                "/blogs",
                &token,
                json!({ "name": "Valid", "slug": slug, "domain": spec }),
            ))
            .await;
        if domain {
            let expected = match spec {
                "Example.com:8080" => "example.com:8080",
                other => other,
            };
            assert_eq!(response.status, StatusCode::CREATED, "{spec}");
            assert_eq!(response.json()["domain"], expected, "{spec}");
        } else {
            response.assert_error(StatusCode::BAD_REQUEST, "invalid input");
        }
    }
}
thread_local! {
    static DOMAIN_SLUG_COUNTER: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
