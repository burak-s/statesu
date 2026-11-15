mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};

use common::{
    TestApp, bearer, create_blog, create_post, json_request, patch_post, register_token,
    set_appearance, set_origins,
};

fn authed_json(method: Method, path: &str, token: &str, body: Value) -> Request<Body> {
    let request = json_request(method, path, body);
    let (mut parts, body) = request.into_parts();
    parts.headers.insert("authorization", bearer(token));
    Request::from_parts(parts, body)
}

/// Bearer-authenticated bodyless request (GET/DELETE). Storage secret
/// routes stay session-only, but uploads and deletes also accept PATs for
/// CLI parity.
fn authed_request(method: Method, path: &str, token: &str) -> Request<Body> {
    let request = common::empty(method, path);
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
async fn blog_delete_is_owner_scoped_and_cascades() {
    let app = TestApp::new().await;
    let owner = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let stranger = register_token(&app, "other@example.com", common::PASSWORD).await;
    let blog = app
        .send(authed_json(
            Method::POST,
            "/blogs",
            &owner,
            json!({ "name": "Delete me", "slug": "delete-me", "domain": "delete.example" }),
        ))
        .await;
    let id = blog.json()["id"].as_str().unwrap().to_owned();
    let path = format!("/blogs/{id}");
    create_post(&app, &owner, &id, json!({ "slug": "post", "title": "Post", "content_markdown": "Body", "status": "published" })).await
        .status_check(StatusCode::CREATED);
    set_origins(&app, &owner, &id, json!(["https://delete.example"]))
        .await
        .status_check(StatusCode::OK);

    common::authed(&app, Method::DELETE, &path, &stranger)
        .await
        .assert_error(StatusCode::NOT_FOUND, "blog not found");
    common::authed(&app, Method::DELETE, "/blogs/not-a-uuid", &owner)
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    common::authed(&app, Method::DELETE, &path, &owner)
        .await
        .status_check(StatusCode::NO_CONTENT);
    common::authed(&app, Method::DELETE, &path, &owner)
        .await
        .assert_error(StatusCode::NOT_FOUND, "blog not found");
    let list = common::authed(&app, Method::GET, "/blogs", &owner).await;
    assert_eq!(list.json(), json!([]));
    common::authed(&app, Method::GET, &format!("{path}/posts"), &owner)
        .await
        .assert_error(StatusCode::NOT_FOUND, "blog not found");

    // The slug and domain are released, and no child records survive reuse.
    let replacement = app
        .send(authed_json(
            Method::POST,
            "/blogs",
            &owner,
            json!({ "name": "Replacement", "slug": "delete-me", "domain": "delete.example" }),
        ))
        .await;
    replacement.status_check(StatusCode::CREATED);
    let replacement_id = replacement.json()["id"].as_str().unwrap().to_owned();
    let posts = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{replacement_id}/posts"),
        &owner,
    )
    .await;
    assert_eq!(posts.json()["items"], json!([]));
    assert_eq!(replacement.json()["origins"], json!([]));
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

/// Saved embed appearance: replace semantics, validation, ownership scoping
/// and that unrelated blog settings survive an appearance write.
#[tokio::test]
async fn embed_appearance_is_saved_cleared_and_owner_scoped() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let id = create_blog(&app, &token, "Blog", "blog").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Unset blogs carry no appearance field at all.
    let fresh = common::authed(&app, Method::GET, "/blogs", &token).await;
    assert!(fresh.json()[0].get("embed_appearance").is_none());

    // A full document is stored, colors normalized, echoes back in blog payloads.
    let saved = set_appearance(
        &app,
        &token,
        &id,
        json!({
            "embed_appearance": {
                "version": 1,
                "colors": { "text": "#1A2B3C", "link": "#7c3aed", "background": "transparent" },
                "typography": { "body_size": 110 },
                "layout": { "max_width": 720, "padding": 24 },
                "decoration": { "corner_radius": 8 },
                "markdown": { "code_background": "#ABC", "table_cell_padding": 12, "quote_border_width": 4,
                    "h1_size": 48, "h1_weight": 600, "link_decoration": "none", "image_align": "right",
                    "table_stripe_background": "#DEF", "inline_code_border_width": 1 }
            }
        }),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text());
    let appearance = saved.json()["embed_appearance"].clone();
    assert_eq!(appearance["version"], 1);
    assert_eq!(appearance["colors"]["text"], "#1a2b3c");
    assert_eq!(appearance["colors"]["background"], "transparent");
    assert_eq!(appearance["typography"]["body_size"], 110);
    assert_eq!(appearance["markdown"]["code_background"], "#aabbcc");
    assert_eq!(appearance["markdown"]["table_cell_padding"], 12);
    assert_eq!(appearance["markdown"]["quote_border_width"], 4);
    assert_eq!(appearance["markdown"]["h1_size"], 48);
    assert_eq!(appearance["markdown"]["h1_weight"], 600);
    assert_eq!(appearance["markdown"]["link_decoration"], "none");
    assert_eq!(appearance["markdown"]["image_align"], "right");
    assert_eq!(appearance["markdown"]["table_stripe_background"], "#ddeeff");
    assert!(appearance["typography"].get("font").is_none());
    assert!(appearance["typography"].get("google_font").is_none());

    let listed = common::authed(&app, Method::GET, "/blogs", &token).await;
    assert_eq!(listed.json()[0]["embed_appearance"], appearance);

    // Replace semantics: the second write fully replaces the first.
    let replaced = set_appearance(
        &app,
        &token,
        &id,
        json!({ "embed_appearance": { "version": 1, "colors": { "link": "#0f0" } } }),
    )
    .await;
    assert_eq!(
        replaced.json()["embed_appearance"]["colors"],
        json!({ "link": "#00ff00" })
    );

    // Keyword defaults mean "use the default": they are stripped at the write
    // edge, so only real values are stored; an all-default document clears.
    let keyworded = set_appearance(
        &app,
        &token,
        &id,
        json!({ "embed_appearance": {
            "version": 1,
            "colors": { "text": "Inherit", "background": "transparent", "link": "revert", "muted": "currentcolor", "border": "rgba(0,0,0,.15)" },
            "typography": { "body_size": 100, "heading_scale": "revert", "line_height": 160 },
            "layout": { "max_width": "none", "padding": 0, "spacing": "revert" },
            "decoration": { "border_width": 0, "corner_radius": 0 },
            "markdown": { "h1_size": "revert", "h1_weight": 700, "code_background": "transparent",
                "code_color": "inherit", "image_align": "center", "link_decoration": "underline" }
        } }),
    )
    .await;
    assert_eq!(keyworded.status, StatusCode::OK, "{}", keyworded.text());
    assert_eq!(
        keyworded.json()["embed_appearance"],
        json!({
            "version": 1,
            "colors": { "background": "transparent", "border": "#00000026" },
            "typography": { "body_size": 100, "line_height": 160 },
            "layout": { "padding": 0 },
            "decoration": { "border_width": 0, "corner_radius": 0 },
            "markdown": { "h1_weight": 700, "image_align": "center", "link_decoration": "underline" }
        })
    );

    // Explicit null clears the saved appearance.
    let cleared = set_appearance(&app, &token, &id, json!({ "embed_appearance": null })).await;
    assert_eq!(cleared.status, StatusCode::OK, "{}", cleared.text());
    assert!(cleared.json().get("embed_appearance").is_none());

    // Out-of-range and malformed values are rejected at validation.
    for bad in [
        json!({ "embed_appearance": { "version": 2 } }),
        json!({ "embed_appearance": { "version": 1, "typography": { "body_size": 500 } } }),
        json!({ "embed_appearance": { "version": 1, "colors": { "link": "red" } } }),
        json!({ "embed_appearance": { "version": 1, "layout": { "max_width": 100 } } }),
        json!({ "embed_appearance": { "version": 1, "markdown": { "code_background": "red" } } }),
        json!({ "embed_appearance": { "version": 1, "markdown": { "table_cell_padding": 97 } } }),
        json!({ "embed_appearance": { "version": 1, "markdown": { "quote_border_width": 9 } } }),
        json!({ "embed_appearance": { "version": 1, "markdown": { "h1_size": 97 } } }),
        json!({ "embed_appearance": { "version": 1, "markdown": { "h1_weight": 99 } } }),
    ] {
        set_appearance(&app, &token, &id, bad)
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
    }
    // Shape violations (unknown or missing fields) are rejected at the body.
    for bad in [
        json!({ "embed_appearance": { "version": 1, "typography": { "font": "serif" } } }),
        json!({ "embed_appearance": { "version": 1, "typography": { "google_font": "Inter" } } }),
        json!({ "embed_appearance": { "version": 1, "css": "body{}" } }),
        json!({ "embed_appearance": {} }),
        json!({}),
    ] {
        set_appearance(&app, &token, &id, bad)
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid body");
    }

    // Ownership is enforced; unknown and foreign blogs read the same.
    let other = register_token(&app, "other@example.com", common::PASSWORD).await;
    let other_id = create_blog(&app, &other, "Other", "other").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    set_appearance(&app, &other, &id, json!({ "embed_appearance": null }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "blog not found");
    set_appearance(&app, &token, &other_id, json!({ "embed_appearance": null }))
        .await
        .assert_error(StatusCode::NOT_FOUND, "blog not found");

    // Appearance writes never disturb unrelated settings.
    set_origins(&app, &token, &id, json!(["https://example.com"])).await;
    set_appearance(
        &app,
        &token,
        &id,
        json!({ "embed_appearance": { "version": 1 } }),
    )
    .await
    .status_check(StatusCode::OK);
    let reread = common::authed(&app, Method::GET, "/blogs", &token).await;
    let blog = reread
        .json()
        .as_array()
        .unwrap()
        .iter()
        .find(|blog| blog["slug"] == "blog")
        .unwrap()
        .clone();
    assert_eq!(blog["origins"], json!(["https://example.com"]));
    assert_eq!(blog["name"], "Blog");
    assert_eq!(blog["embed_appearance"]["version"], 1);
}

/// Named appearance variants: snapshot the current look or an explicit
/// document, list newest first, apply, delete; names are unique per blog and
/// ownership is enforced without leaking other accounts' variants.
#[tokio::test]
async fn appearance_variants_are_named_snapshots_applied_and_owner_scoped() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let id = create_blog(&app, &token, "Styles", "styles").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Snapshot without a document: nothing is saved yet, so it is rejected.
    app.send(authed_json(
        Method::POST,
        &format!("/blogs/{id}/appearance/variants"),
        &token,
        json!({ "name": "First" }),
    ))
    .await
    .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    // Save the current look under a name after styling the blog.
    set_appearance(
        &app,
        &token,
        &id,
        json!({ "embed_appearance": { "version": 1, "colors": { "link": "#123456" } } }),
    )
    .await;
    let first = app
        .send(authed_json(
            Method::POST,
            &format!("/blogs/{id}/appearance/variants"),
            &token,
            json!({ "name": " Brand look " }),
        ))
        .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.text());
    assert_eq!(first.json()["name"], "Brand look");
    assert_eq!(
        first.json()["embed_appearance"]["colors"]["link"],
        "#123456"
    );
    let first_id = first.json()["id"].as_str().unwrap().to_owned();

    // An explicit document snapshots that document instead; keyword
    // defaults are stripped like every appearance write.
    let second = app
        .send(authed_json(
            Method::POST,
            &format!("/blogs/{id}/appearance/variants"),
            &token,
            json!({ "name": "Cozy", "embed_appearance": { "version": 1, "typography": { "body_size": "revert" }, "layout": { "padding": 24 } } }),
        ))
        .await;
    assert_eq!(second.status, StatusCode::CREATED, "{}", second.text());
    assert_eq!(
        second.json()["embed_appearance"],
        json!({ "version": 1, "layout": { "padding": 24 } })
    );

    // Duplicate names conflict; empty and oversized names are invalid.
    for (body, status, message) in [
        (
            json!({ "name": "cozy" }),
            StatusCode::CONFLICT,
            "style name already in use",
        ),
        (
            json!({ "name": "  " }),
            StatusCode::BAD_REQUEST,
            "invalid input",
        ),
        (
            json!({ "name": "x".repeat(65) }),
            StatusCode::BAD_REQUEST,
            "invalid input",
        ),
    ] {
        app.send(authed_json(
            Method::POST,
            &format!("/blogs/{id}/appearance/variants"),
            &token,
            body,
        ))
        .await
        .assert_error(status, message);
    }

    // Listing is newest first and carries the documents.
    let listed = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{id}/appearance/variants"),
        &token,
    )
    .await;
    assert_eq!(listed.json().as_array().unwrap().len(), 2);
    assert_eq!(listed.json()[0]["name"], "Cozy");
    assert_eq!(listed.json()[1]["id"], first_id);

    // Applying a snapshot replaces the live appearance (keyword defaults
    // included in the stored doc keep their stripped shape).
    let applied = set_appearance(
        &app,
        &token,
        &id,
        json!({ "embed_appearance": listed.json()[0]["embed_appearance"].clone() }),
    )
    .await;
    assert_eq!(
        applied.json()["embed_appearance"],
        json!({ "version": 1, "layout": { "padding": 24 } })
    );

    // Ownership: another account sees no variants and cannot mutate them.
    let other = register_token(&app, "variants-other@example.com", common::PASSWORD).await;
    let other_id = create_blog(&app, &other, "Other", "other").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{other_id}/appearance/variants"),
        &other,
    )
    .await
    .status_check(StatusCode::OK);
    common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{id}/appearance/variants"),
        &other,
    )
    .await
    .assert_error(StatusCode::NOT_FOUND, "blog not found");
    common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{other_id}/appearance/variants/{first_id}"),
        &other,
    )
    .await
    .assert_error(StatusCode::NOT_FOUND, "blog not found");

    // Deleting removes exactly one variant; unknown ids read as 404.
    let deleted = common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{id}/appearance/variants/{first_id}"),
        &token,
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    common::authed(
        &app,
        Method::DELETE,
        &format!("/blogs/{id}/appearance/variants/{first_id}"),
        &token,
    )
    .await
    .assert_error(StatusCode::NOT_FOUND, "blog not found");
    let listed = common::authed(
        &app,
        Method::GET,
        &format!("/blogs/{id}/appearance/variants"),
        &token,
    )
    .await;
    assert_eq!(listed.json().as_array().unwrap().len(), 1);
    assert_eq!(listed.json()[0]["name"], "Cozy");
}

/// Cross-checks the management endpoint against the Rust validator for every
/// numeric token at both bounds and just outside them, plus wrong wire types,
/// so a frontend edit cannot drift past the backend undetected.
#[tokio::test]
async fn embed_appearance_numeric_bounds_and_wire_types_match_the_validator() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let id = create_blog(&app, &token, "Bounds", "bounds").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let fields = [
        ("typography", "body_size", 75, 200),
        ("typography", "heading_scale", 100, 225),
        ("typography", "line_height", 100, 250),
        ("layout", "max_width", 320, 1200),
        ("layout", "padding", 0, 96),
        ("layout", "spacing", 0, 96),
        ("decoration", "border_width", 0, 8),
        ("decoration", "corner_radius", 0, 32),
    ];
    for (group, key, min, max) in fields {
        let document = |value: serde_json::Value| {
            json!({
                "embed_appearance": {
                    "version": 1,
                    group: { key: value }
                }
            })
        };
        set_appearance(&app, &token, &id, document(json!(min)))
            .await
            .status_check(StatusCode::OK);
        set_appearance(&app, &token, &id, document(json!(max)))
            .await
            .status_check(StatusCode::OK);
        // Values outside the integer wire type (strings, floats, negatives)
        // fail deserialization; in-type overflow fails validation.
        let outside_error = if min == 0 {
            "invalid body"
        } else {
            "invalid input"
        };
        set_appearance(&app, &token, &id, document(json!(min - 1)))
            .await
            .assert_error(StatusCode::BAD_REQUEST, outside_error);
        set_appearance(&app, &token, &id, document(json!(max + 1)))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
        set_appearance(&app, &token, &id, document(json!("100")))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid body");
        set_appearance(&app, &token, &id, document(json!(true)))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid body");
        set_appearance(&app, &token, &id, document(json!([100])))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid body");
        set_appearance(&app, &token, &id, document(json!(min as f64 + 0.5)))
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid body");
    }
}

/// A rejected appearance write must never modify the stored value, and blog
/// management writes must never touch (or clear) the saved appearance.
#[tokio::test]
async fn embed_appearance_is_immutable_under_rejections_and_unrelated_writes() {
    let app = TestApp::new().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let id = create_blog(&app, &token, "Stable", "stable").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let appearance = json!({
        "version": 1,
        "colors": { "text": "#123456", "link": "#7c3aed" },
        "layout": { "max_width": 720 }
    });
    set_appearance(&app, &token, &id, json!({ "embed_appearance": appearance }))
        .await
        .status_check(StatusCode::OK);

    for bad in [
        json!({ "embed_appearance": { "version": 2 } }),
        json!({ "embed_appearance": { "version": 1, "colors": { "link": "red" } } }),
    ] {
        set_appearance(&app, &token, &id, bad)
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
        let listed = common::authed(&app, Method::GET, "/blogs", &token).await;
        assert_eq!(
            listed.json()[0]["embed_appearance"],
            appearance,
            "a rejected write must not change the stored value"
        );
    }
    for bad in [
        json!({ "embed_appearance": { "version": 1, "typography": { "body_size": 500 } } }),
        json!({ "embed_appearance": { "version": 1, "colors": { "link": "red" } } }),
    ] {
        set_appearance(&app, &token, &id, bad)
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid input");
        let listed = common::authed(&app, Method::GET, "/blogs", &token).await;
        assert_eq!(
            listed.json()[0]["embed_appearance"],
            appearance,
            "a rejected write must not change the stored value"
        );
    }
    for bad in [json!({ "embed_appearance": { "version": 1, "css": "body{}" } })] {
        set_appearance(&app, &token, &id, bad)
            .await
            .assert_error(StatusCode::BAD_REQUEST, "invalid body");
        let listed = common::authed(&app, Method::GET, "/blogs", &token).await;
        assert_eq!(
            listed.json()[0]["embed_appearance"],
            appearance,
            "a rejected write must not change the stored value"
        );
    }

    // Blog management writes leave the appearance alone.
    app.send(authed_json(
        Method::PATCH,
        &format!("/blogs/{id}"),
        &token,
        json!({ "name": "Renamed", "domain": null }),
    ))
    .await
    .status_check(StatusCode::OK);
    let reread = common::authed(&app, Method::GET, "/blogs", &token).await;
    assert_eq!(reread.json()[0]["name"], "Renamed");
    assert_eq!(
        reread.json()[0]["embed_appearance"]["colors"]["text"],
        "#123456"
    );

    // Appearance writes leave blog management fields alone.
    set_appearance(
        &app,
        &token,
        &id,
        json!({ "embed_appearance": { "version": 1 } }),
    )
    .await
    .status_check(StatusCode::OK);
    let final_list = common::authed(&app, Method::GET, "/blogs", &token).await;
    assert_eq!(final_list.json()[0]["name"], "Renamed");
    assert_eq!(final_list.json()[0]["domain"], Value::Null);
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

#[tokio::test]
async fn content_quota_caps_posts_and_bytes_per_owner() {
    // Post-count cap counts soft-deleted posts and is shared across blogs.
    let app = TestApp::build(|config| config.max_posts_per_user = 2).await;
    let token = register_token(&app, "quota-count@example.com", "countingishard7").await;
    let blog = create_blog(&app, &token, "Count", "count").await;
    let blog_id = blog.json()["id"].as_str().unwrap().to_owned();
    for index in 0..2 {
        let created = create_post(
            &app,
            &token,
            &blog_id,
            json!({ "slug": format!("p{index}"), "title": "t", "content_markdown": "x" }),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    }
    let third = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "p2", "title": "t", "content_markdown": "x" }),
    )
    .await;
    third.assert_error(StatusCode::FORBIDDEN, "forbidden");
    // A second blog shares the same owner budget.
    let other = create_blog(&app, &token, "Other", "other").await;
    third.assert_error(StatusCode::FORBIDDEN, "forbidden");
    let other_id = other.json()["id"].as_str().unwrap().to_owned();
    let overflow = create_post(
        &app,
        &token,
        &other_id,
        json!({ "slug": "p2", "title": "t", "content_markdown": "x" }),
    )
    .await;
    overflow.assert_error(StatusCode::FORBIDDEN, "forbidden");

    // Byte cap: drafts, published and soft-deleted Markdown all count.
    let app = TestApp::build(|config| {
        config.max_posts_per_user = 10;
        config.max_total_content_bytes = 160 * 1024;
    })
    .await;
    let token = register_token(&app, "quota-bytes@example.com", "bytesmatter7").await;
    let blog = create_blog(&app, &token, "Bytes", "bytes").await;
    let blog_id = blog.json()["id"].as_str().unwrap().to_owned();
    let chunk = "a".repeat(60 * 1024);
    let id_of = |slug: &str, listed: Value| -> String {
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["slug"] == slug)
            .expect("post present in listing")["id"]
            .as_str()
            .expect("id")
            .to_owned()
    };

    let first = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "b1", "title": "t", "content_markdown": chunk.clone() }),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.text());
    let second = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "b2", "title": "t", "content_markdown": chunk.clone() }),
    )
    .await;
    assert_eq!(second.status, StatusCode::CREATED, "{}", second.text());
    // 180 KiB > 160 KiB cap.
    let third = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "b3", "title": "t", "content_markdown": chunk.clone() }),
    )
    .await;
    third.assert_error(StatusCode::FORBIDDEN, "forbidden");

    // Soft-deleting does not free the byte budget: the deleted chunk stays
    // counted, so the next create is still over the cap.
    let listing = app
        .send(authed_json(
            Method::GET,
            &format!("/blogs/{blog_id}/posts"),
            &token,
            json!({}),
        ))
        .await
        .json();
    let b2_id = id_of("b2", listing.clone());
    let deleted = app
        .send(authed_json(
            Method::DELETE,
            &format!("/blogs/{blog_id}/posts/{b2_id}"),
            &token,
            json!({}),
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let again = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "b4", "title": "t", "content_markdown": chunk.clone() }),
    )
    .await;
    again.assert_error(StatusCode::FORBIDDEN, "forbidden");

    // Shrinking an existing post always stays possible, then frees budget.
    let b1_id = id_of("b1", listing);
    let shrink = patch_post(
        &app,
        &token,
        &blog_id,
        &b1_id,
        json!({ "content_markdown": "a".repeat(20 * 1024) }),
    )
    .await;
    assert_eq!(shrink.status, StatusCode::OK, "{}", shrink.text());
    // 60 KiB (deleted, still counted) + 20 KiB + 60 KiB = 140 KiB.
    let freed = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "b5", "title": "t", "content_markdown": chunk }),
    )
    .await;
    assert_eq!(freed.status, StatusCode::CREATED, "{}", freed.text());
    let new_listing = app
        .send(authed_json(
            Method::GET,
            &format!("/blogs/{blog_id}/posts"),
            &token,
            json!({}),
        ))
        .await
        .json();
    let b5_id = id_of("b5", new_listing);
    // 140 KiB - 60 KiB + 85 KiB = 165 KiB > 160 KiB cap.
    let grown = patch_post(
        &app,
        &token,
        &blog_id,
        &b5_id,
        json!({ "content_markdown": "a".repeat(85 * 1024) }),
    )
    .await;
    grown.assert_error(StatusCode::FORBIDDEN, "forbidden");
}

/// The server stores untrusted Markdown verbatim and never processes it as
/// HTML, script or markup: attack payloads must survive create/patch, appear
/// byte-for-byte over both management and public reads, and any later
/// execution risk stays entirely with the browser-side renderer.
#[tokio::test]
async fn stored_attack_payload_round_trips_verbatim() {
    let app = TestApp::new().await;
    let token = register_token(&app, "payload@example.com", "intheclear7").await;
    let blog = create_blog(&app, &token, "Payload", "payload").await;
    let blog_id = blog.json()["id"].as_str().unwrap().to_owned();
    let attacks = "# hi\n\n<script>alert('x')</script>\n<img src=x onerror=alert(1)>\n<iframe src=\"evil.example\"></iframe>\n[link](javascript:alert(1)) [data](data:text/html,<script>alert(2)</script>)\n<body onload=alert(3)> <svg><script>alert(4)</script></svg>\n{\"payload\":\"value\"}";
    let created = create_post(
        &app,
        &token,
        &blog_id,
        json!({
            "slug": "attack",
            "title": "Attack",
            "content_markdown": attacks,
            "status": "published"
        }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());

    let management = app
        .send(authed_json(
            Method::GET,
            &format!("/blogs/{blog_id}/posts"),
            &token,
            json!({}),
        ))
        .await;
    assert_eq!(management.json()["items"][0]["content_markdown"], attacks);

    let public_read = app
        .send(empty_request(
            Method::GET,
            "/public/blogs/payload/posts/attack",
        ))
        .await;
    assert_eq!(public_read.status, StatusCode::OK);
    assert_eq!(public_read.json()["content_markdown"], attacks);
    assert_eq!(public_read.header("content-type"), Some("application/json"));
    assert_eq!(
        public_read.header("x-content-type-options"),
        Some("nosniff")
    );
}

fn empty_request(method: Method, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request")
}

/// A quota that was lowered below existing storage must not lock accounts:
/// non-growing updates are accepted whatever the stored total is.
#[tokio::test]
async fn shrinks_stay_possible_when_usage_is_over_the_cap() {
    let app = TestApp::build(|config| {
        config.max_posts_per_user = 10;
        config.max_total_content_bytes = 32 * 1024;
    })
    .await;
    let token = register_token(&app, "over-cap@example.com", "stucknot7").await;
    let blog = create_blog(&app, &token, "OverCap", "over-cap").await;
    let blog_id = blog.json()["id"].as_str().unwrap().to_owned();
    let created = create_post(
        &app,
        &token,
        &blog_id,
        json!({ "slug": "c1", "title": "t", "content_markdown": "a".repeat(12 * 1024) }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let post_id = created.json()["id"].as_str().unwrap().to_owned();
    // Bypass the service the way a lowered config would: the stored content
    // (40 KiB) now exceeds the account cap (32 KiB).
    sqlx::query("UPDATE posts SET content_markdown = ? WHERE id = ?")
        .bind("a".repeat(40 * 1024))
        .bind(&post_id)
        .execute(&app.app.pool)
        .await
        .expect("raw pad write");
    // Growth is refused. Shrinking must succeed even while the stored total
    // stays over the cap (40 -> 36 KiB, still above 32 KiB): the old guard
    // rejected exactly this case and locked the account.
    let grown = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "a".repeat(41 * 1024) }),
    )
    .await;
    grown.assert_error(StatusCode::FORBIDDEN, "forbidden");
    let partial = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "a".repeat(36 * 1024) }),
    )
    .await;
    assert_eq!(partial.status, StatusCode::OK, "{}", partial.text());
    let again_over = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "a".repeat(40 * 1024) }),
    )
    .await;
    again_over.assert_error(StatusCode::FORBIDDEN, "forbidden");
    let shrink = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "a".repeat(10 * 1024) }),
    )
    .await;
    assert_eq!(shrink.status, StatusCode::OK, "{}", shrink.text());
    // Once under the cap, the freed budget works as normal.
    let refill = patch_post(
        &app,
        &token,
        &blog_id,
        &post_id,
        json!({ "content_markdown": "a".repeat(32 * 1024) }),
    )
    .await;
    assert_eq!(refill.status, StatusCode::OK, "{}", refill.text());
}

/// The quota statement is atomic: racing creates against a tight cap yield
/// exactly one success, never an overshoot.
#[tokio::test]
async fn concurrent_creates_cannot_overshoot_the_post_cap() {
    let app = TestApp::build(|config| config.max_posts_per_user = 1).await;
    let token = register_token(&app, "race@example.com", "onewriter7").await;
    let blog = create_blog(&app, &token, "Race", "race").await;
    let blog_id = blog.json()["id"].as_str().unwrap().to_owned();
    let make = |slug: String| {
        create_post(
            &app,
            &token,
            &blog_id,
            json!({ "slug": slug, "title": "t", "content_markdown": "x" }),
        )
    };
    let first_request = make;
    let (first, second) = tokio::join!(make("p1".to_owned()), make("p2".to_owned()));
    let _ = first_request;
    let successes = [&first, &second]
        .iter()
        .filter(|response| response.status == StatusCode::CREATED)
        .count();
    let refusals = [&first, &second]
        .iter()
        .filter(|response| response.status == StatusCode::FORBIDDEN)
        .count();
    assert_eq!(successes, 1, "exactly one create wins the cap");
    assert_eq!(refusals, 1, "exactly one create is refused");
}
// -------------------------------------------------------------------------
// Media upload pipeline (server bucket; pipeline bytes; quota; types)
// -------------------------------------------------------------------------

use common::{TestResponse, create_pat, login_session, session_authorized};

/// Registers and verifies an account, then opens its session.
async fn setup_session(app: &TestApp, email: &str) -> (String, String) {
    common::register_and_verify(app, email, common::PASSWORD).await;
    login_session(app, email, common::PASSWORD).await
}

/// Small deterministic 800x400 PNG for the pipeline tests.
fn media_png_bytes() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(800, 400, image::Rgba([12, 90, 210, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("fixture encode");
    bytes
}

/// Bearer-upload of raw body bytes to a blog's media route.
async fn upload_raw(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    media_bytes: Vec<u8>,
) -> TestResponse {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", bearer(token))
        .header("content-type", "application/octet-stream")
        .body(Body::from(media_bytes))
        .expect("upload request");
    app.send(request).await
}

#[tokio::test]
async fn media_upload_processes_stores_and_lists() {
    let (app, _storage) = TestApp::with_server_storage().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Media Blog", "media-blog")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let created = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        media_png_bytes(),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let body = created.json();
    assert_eq!(body["status"], "ready");
    assert_eq!(body["mime"], "image/png");
    assert_eq!(body["width"], 800);
    assert_eq!(body["height"], 400);
    // The public URL carries the server bucket's public base and the key.
    let public_url = body["public_url"].as_str().unwrap();
    assert!(
        public_url.starts_with("https://media.test.example/statesu/blogs/"),
        "unexpected public url {public_url}"
    );
    assert!(public_url.ends_with("/source.png"));
    // An 800-wide source produces exactly the smaller variants.
    let variant_labels: Vec<&str> = body["variants"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|variant| variant["label"].as_str())
        .collect();
    assert_eq!(variant_labels, vec!["w640"]);
    let variant = &body["variants"][0];
    assert_eq!(variant["width"], 640);
    assert_eq!(variant["height"], 320);
    assert!(
        variant["public_url"]
            .as_str()
            .unwrap()
            .ends_with("/w640.png")
    );
    assert!(body["stored_bytes"].as_i64().unwrap() > 0);

    // The listing mirrors the same record.
    let list = app
        .send(authed_request(
            Method::GET,
            &format!("/blogs/{blog_id}/media"),
            &token,
        ))
        .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text());
    let listing = list.json();
    assert_eq!(listing["total"], 1);
    assert_eq!(listing["items"][0]["id"], body["id"]);
    assert_eq!(listing["items"][0]["public_url"], body["public_url"]);
    assert_eq!(listing["items"][0]["variants"][0]["width"], 640);
}

#[tokio::test]
async fn media_upload_rejects_bad_bodies_and_oversize() {
    let (app, _storage) = TestApp::with_server_storage().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Media Bad", "media-bad")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let upload_path = format!("/blogs/{blog_id}/media");

    // Empty body.
    upload_raw(&app, Method::POST, &upload_path, &token, Vec::new())
        .await
        .assert_error(StatusCode::BAD_REQUEST, "invalid input");

    // Non-image bytes.
    upload_raw(
        &app,
        Method::POST,
        &upload_path,
        &token,
        b"definitely not an image".to_vec(),
    )
    .await
    .assert_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported media type");

    // Oversize: the body limit is the configuration ceiling.
    let oversized = upload_raw(
        &app,
        Method::POST,
        &upload_path,
        &token,
        vec![0u8; 64 * 1024 + 1],
    )
    .await;
    assert!(
        oversized.status == StatusCode::BAD_REQUEST
            || oversized.status == StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        oversized.text()
    );
}

#[tokio::test]
async fn media_quota_cap_refuses_over_budget() {
    let (app, _storage) = TestApp::with_configured_storage(|config| {
        config.r2_default = Some(TestApp::test_server_storage());
        config.max_media_per_user = 2;
    })
    .await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Media Cap", "media-cap")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for _ in 0..2 {
        let created = upload_raw(
            &app,
            Method::POST,
            &format!("/blogs/{blog_id}/media"),
            &token,
            media_png_bytes(),
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    }
    upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        media_png_bytes(),
    )
    .await
    .assert_error(StatusCode::FORBIDDEN, "forbidden");
}

#[tokio::test]
async fn media_ownership_and_delete_lifecycle() {
    let (app, _storage) = TestApp::with_server_storage().await;
    let owner = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let stranger = register_token(&app, "media-stranger@example.com", common::PASSWORD).await;
    let blog_id = create_blog(&app, &owner, "Media Own", "media-own")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let created = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &owner,
        media_png_bytes(),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let media_id = created.json()["id"].as_str().unwrap().to_owned();
    let media_path = format!("/blogs/{blog_id}/media/{media_id}");

    // Foreign user cannot even list the blog's media.
    let foreign = app
        .send(authed_request(
            Method::GET,
            &format!("/blogs/{blog_id}/media"),
            &stranger,
        ))
        .await;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND, "{}", foreign.text());
    assert_eq!(foreign.json()["error"], "blog not found");
    let foreign_delete = app
        .send(authed_request(Method::DELETE, &media_path, &stranger))
        .await;
    foreign_delete.assert_error(StatusCode::NOT_FOUND, "blog not found");

    // Owner deletes for real.
    let deleted = app
        .send(authed_request(Method::DELETE, &media_path, &owner))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    app.send(authed_request(Method::DELETE, &media_path, &owner))
        .await
        .assert_error(StatusCode::NOT_FOUND, "media not found");
    let list = app
        .send(authed_request(
            Method::GET,
            &format!("/blogs/{blog_id}/media"),
            &owner,
        ))
        .await;
    assert_eq!(list.json()["total"], 0);
}
/// A real half-write: the source lands on the bucket, the variant write
/// fails, the request dies. The bucket keeps the first object until the
/// pre-registered cleanup job runs, at which point every object is gone —
/// including the one that was successful.
#[tokio::test]
async fn interrupted_upload_cleans_bucket_through_maintenance() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Halfway", "halfway").await.json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // First write succeeds (the source), the variant write rejects, and
    // the request dies exactly there.
    storage.fail_puts_after(1);
    let upload = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        media_png_bytes(),
    )
    .await;
    assert_ne!(upload.status, StatusCode::CREATED, "{}", upload.text());
    let events = storage.events();
    let put_attempts: Vec<&str> = events
        .iter()
        .filter_map(|event| event.strip_prefix("put:"))
        .collect();
    let landed: Vec<&str> = put_attempts
        .iter()
        .copied()
        .filter(|key| storage.object(key).is_some())
        .collect();
    assert_eq!(landed.len(), 1, "half-write: {:?}", put_attempts);
    let source_key = landed[0].to_owned();
    assert!(storage.object(&source_key).is_some(), "the source landed");
    assert!(source_key.ends_with("/source.png"));

    // A failed upload keeps an error row for visibility while releasing
    // quota; maintenance clears the bucket side independently.
    let failed = app
        .send(authed_request(
            Method::GET,
            &format!("/blogs/{blog_id}/media"),
            &token,
        ))
        .await;
    assert_eq!(
        failed.json()["items"][0]["status"],
        "error",
        "{}",
        failed.text()
    );

    app.clock.advance(400);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("cleaning");
    assert!(
        storage.object(&source_key).is_none(),
        "the landed object must be cleaned once maintenance runs"
    );
    let deletions: Vec<String> = storage
        .events()
        .iter()
        .filter_map(|event| event.strip_prefix("delete:"))
        .map(str::to_owned)
        .collect();
    assert!(
        deletions.iter().any(|key| key.contains("/w640")),
        "variant candidate paths ride in the cleanup payload: {:?}",
        deletions
    );
    assert!(
        deletions.contains(&source_key),
        "the landed source key is addressed: {:?}",
        deletions
    );
}

#[tokio::test]
async fn deleting_media_survives_delete_failures_until_maintenance() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Delete Retry", "media-retry-delete")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        media_png_bytes(),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let media_id = created.json()["id"].as_str().unwrap().to_owned();
    let public_url = created.json()["public_url"].as_str().unwrap().to_owned();
    let object_key = public_url
        .strip_prefix("https://media.test.example/")
        .expect("server bucket URL")
        .to_owned();

    // Deletes fail inline; the request leaves the job behind, public reads
    // drop the record at once, and maintenance finishes the removal.
    storage.fail_deletes(2);
    let deleted = app
        .send(authed_request(
            Method::DELETE,
            &format!("/blogs/{blog_id}/media/{media_id}"),
            &token,
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    assert!(
        storage.object(&object_key).is_some(),
        "object waits for the durable job completion"
    );
    let list = app
        .send(authed_request(
            Method::GET,
            &format!("/blogs/{blog_id}/media"),
            &token,
        ))
        .await;
    assert_eq!(
        list.json()["total"],
        0,
        "deleting rows hide from lists: {}",
        list.text()
    );

    app.clock.advance(400);
    storage.fail_deletes(2);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("cleaning");

    assert!(
        storage.object(&object_key).is_some(),
        "a failed maintenance pass must not claim success"
    );
    app.clock.advance(120);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("retry pass");
    assert!(storage.object(&object_key).is_none());
}

#[tokio::test]
async fn blog_deletion_enqueues_bucket_cleanup() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Cascade", "media-cascade")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let created = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        media_png_bytes(),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let public_url = created.json()["public_url"].as_str().unwrap().to_owned();
    let object_key = public_url
        .strip_prefix("https://media.test.example/")
        .expect("server bucket URL")
        .to_owned();
    assert!(storage.object(&object_key).is_some());

    let response = app
        .send(authed_request(
            Method::DELETE,
            &format!("/blogs/{blog_id}"),
            &token,
        ))
        .await;
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "{}",
        response.text()
    );
    app.clock.advance(40);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("cleaning");
    app.clock.advance(120);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("retry");
    assert!(
        storage.object(&object_key).is_none(),
        "blog deletion reaches into the bucket"
    );
}

// -------------------------------------------------------------------------
#[tokio::test]
async fn media_upload_honors_configured_byte_limit_not_extractor_default() {
    // Bytes planned of the request goes fully through: the default 2 MiB
    // extractor limit must not clip a 3 MiB body when the configured file
    // cap is 4 MiB.
    let readable_size = 3 * 1024 * 1024 + 2044usize;
    let (app, _storage) = TestApp::with_configured_storage(|config| {
        config.max_media_bytes = 4 * 1024 * 1024usize;
        config.r2_default = Some(TestApp::test_server_storage());
    })
    .await;
    let token = register_token(&app, common::EMAIL, common::PASSWORD).await;
    let blog_id = create_blog(&app, &token, "Wrong Size", "media-size")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    // A real PNG too large for the default floor, still under the config.
    let oversized = {
        use image::{ImageBuffer, Rgba};
        let wide = ImageBuffer::from_pixel(20, 20, Rgba([255u8, 0, 0, 255]));
        let mut data = Vec::new();
        wide.write_to(
            &mut std::io::Cursor::new(&mut data),
            image::ImageFormat::Png,
        )
        .expect("fixture encode");
        data
    };
    let padded = {
        // Bloat the byte stream past 3 MiB while keeping the format valid:
        // append filler bytes (the pipeline re-encodes anyway).
        let mut data = oversized;
        let filler = readable_size.saturating_sub(data.len());
        data.extend(std::iter::repeat_n(0u8, filler));
        data
    };
    let uploaded = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        padded,
    )
    .await;
    assert_eq!(
        uploaded.status,
        StatusCode::CREATED,
        "3 MiB upload with 4 MiB limit must pass, not hit an extractor default: {}",
        uploaded.text()
    );
}

/// Server-bucket rows carry the bucket identity — never the credentials:
/// when the identity still exists in config, cleanup re-resolves the
/// CURRENT (rotated) credentials; a moved bucket identity is never given
/// guessed keys and its object stays with a visible retry record.
#[tokio::test]
async fn cleanup_resolves_server_identity_with_current_credentials() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let me = app
        .send(session_authorized(
            common::empty(Method::GET, "/auth/me"),
            &cookie,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let owner_id = me.json()["id"].as_str().unwrap().to_string();
    let token = common::create_pat(&app, &cookie, &csrf, true).await;
    let blog_id = create_blog(&app, &token, "Snapshot", "media-snapshot")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let location_of = |bucket: &str, prefix: &str| {
        serde_json::json!({
            "account_id": "abcdef0123456789abcdef0123456789",
            "bucket": bucket,
            "prefix": prefix,
            "public_base_url": "https://media.test.example",
        })
    };
    // Row A: identity matches today's config (same bucket); its objects
    // clean with the environment credentials, whatever rotation happened.
    let row_a = uuid::Uuid::now_v7();
    let key_a = "statesu/blogs/snap/a/source.png";
    let row_b = uuid::Uuid::now_v7();
    let key_b = "oldprefix/blogs/snap/abc/source.png";
    // A committed row that predates a config change: its bytes live on
    // "old-bucket", the current app config points at "statesu-bucket".
    let mut tx = app.app.pool.begin().await.expect("tx");
    for (id, bucket, prefix, key) in [
        (row_a, "statesu-bucket", "statesu", key_a),
        (row_b, "old-bucket", "oldprefix", key_b),
    ] {
        let location_blob = location_of(bucket, prefix).to_string().into_bytes();
        sqlx::query(
            "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, \
             storage_location, status, mime, stored_bytes, reserved_bytes, created_at, updated_at) \
             VALUES (?, ?, ?, NULL, 'https://media.test.example', ?, 'ready', 'image/png', 12, 0, ?, ?)",
        )
        .bind(id.to_string())
        .bind(blog_id.clone())
        .bind(owner_id.clone())
        .bind(location_blob)
        .bind(common::NOW)
        .bind(common::NOW)
        .execute(&mut *tx)
        .await
        .expect("seed media row");
        sqlx::query(
            "INSERT INTO media_variants (media_id, label, object_key, mime, width, height, bytes) \
             VALUES (?, 'source', ?, 'image/png', 20, 20, 12)",
        )
        .bind(id.to_string())
        .bind(key)
        .execute(&mut *tx)
        .await
        .expect("seed variant row");
    }
    tx.commit().await.expect("commit");
    for key in [key_a, key_b] {
        statesu::storage::StorageFactory::store_for(
            storage.as_ref(),
            &statesu::storage::BucketConnection {
                account_id: "seed-account".to_owned(),
                bucket: "seed-bucket".to_owned(),
                prefix: String::new(),
                access_key_id: "seed-key".to_owned(),
                secret_key: "seed-secret".to_owned(),
            },
        )
        .expect("store")
        .put(key, b"stale object")
        .await
        .expect("seed object");
    }

    // Deleting both rows: A resolves via the current config; B cannot
    // (its bucket moved) and refuses rather than run with wrong keys.
    for row in [row_a, row_b] {
        let deleted = app
            .send(authed_request(
                Method::DELETE,
                &format!("/blogs/{blog_id}/media/{row}"),
                &token,
            ))
            .await;
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    }
    assert!(
        storage.object(key_a).is_none(),
        "matching identity cleans with current credentials"
    );
    assert!(
        storage.object(key_b).is_some(),
        "moved bucket identity never deletes blind"
    );
    let used = storage.used_credentials();
    assert!(
        used.iter()
            .any(|entry| entry.ends_with("/statesu-bucket/server-key")),
        "matching identity uses the CURRENT credentials: {used:?}"
    );
    assert!(
        !used.iter().any(|entry| entry.contains("old-bucket")),
        "moved bucket identities are never addressed: {used:?}"
    );

    // B's cleanup keeps retrying visibly; nothing magically deletes.
    app.clock.advance(400);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    assert!(
        storage.object(key_b).is_some(),
        "moved bucket objects are left for operator migration"
    );
}

/// A single media delete on unresolvable storage is never a fake 204:
/// the record leaves the public surface in the same request and the
/// object paths stay durable in a cleanup job until the storage identity
/// returns; a config restore then completes the deletion itself.
#[tokio::test]
async fn media_delete_on_unresolvable_storage_leaves_a_durable_wait() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let token = common::create_pat(&app, &cookie, &csrf, true).await;
    let blog_id = create_blog(&app, &token, "Wait", "media-delete-wait")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let me = app
        .send(session_authorized(
            common::empty(Method::GET, "/auth/me"),
            &cookie,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let owner_uuid = uuid::Uuid::parse_str(me.json()["id"].as_str().unwrap()).expect("owner uuid");
    let blog_uuid = uuid::Uuid::parse_str(&blog_id).expect("blog uuid");
    let row = uuid::Uuid::now_v7();
    let stale_key = "moved-bucket/blogs/wait/abc/source.png";
    let seed_location = serde_json::json!({
        "account_id": "abcdef0123456789abcdef0123456789",
        "bucket": "statesu-bucket",
        "prefix": "statesu",
        "public_base_url": "https://media.test.example",
    })
    .to_string()
    .into_bytes();
    sqlx::query(
        "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, \
         storage_location, status, mime, stored_bytes, reserved_bytes, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, 'https://media.test.example', ?, 'ready', 'image/png', 12, 0, ?, ?)",
    )
    .bind(row.to_string())
    .bind(blog_uuid.to_string())
    .bind(owner_uuid.to_string())
    .bind(seed_location)
    .bind(common::NOW)
    .bind(common::NOW)
    .execute(&app.app.pool)
    .await
    .expect("wait media row");
    sqlx::query(
        "INSERT INTO media_variants (media_id, label, object_key, mime, width, height, bytes) \
         VALUES (?, 'source', ?, 'image/png', 20, 20, 12)",
    )
    .bind(row.to_string())
    .bind(stale_key)
    .execute(&app.app.pool)
    .await
    .expect("wait variant row");
    statesu::storage::StorageFactory::store_for(
        storage.as_ref(),
        &statesu::storage::BucketConnection {
            account_id: "abcdef0123456789abcdef0123456789".to_owned(),
            bucket: "statesu-bucket".to_owned(),
            prefix: "statesu".to_owned(),
            access_key_id: "server-key".to_owned(),
            secret_key: "server-secret".to_owned(),
        },
    )
    .expect("store")
    .put(stale_key, b"waited object")
    .await
    .expect("seed object");

    // Then the location snapshot is moved to a bucket today's config
    // cannot serve; the delete answers 204 by taking the durable path,
    // not by faking success.
    sqlx::query("UPDATE media SET storage_location = ? WHERE id = ?")
        .bind(
            serde_json::json!({
                "account_id": "abcdef0123456789abcdef0123456789",
                "bucket": "moved-bucket",
                "prefix": "moved",
                "public_base_url": "https://media.test.example",
            })
            .to_string()
            .into_bytes(),
        )
        .bind(row.to_string())
        .execute(&app.app.pool)
        .await
        .expect("moved location");
    let deleted = app
        .send(authed_request(
            Method::DELETE,
            &format!("/blogs/{blog_id}/media/{row}"),
            &token,
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    let status: String = sqlx::query_scalar("SELECT status FROM media WHERE id = ?")
        .bind(row.to_string())
        .fetch_one(&app.app.pool)
        .await
        .expect("row status");
    assert_eq!(
        status, "deleting",
        "the row left the public surface at once"
    );
    let job_payload: String =
        sqlx::query_scalar("SELECT payload FROM media_jobs ORDER BY created_at LIMIT 1")
            .fetch_one(&app.app.pool)
            .await
            .expect("durable job");
    let parsed: serde_json::Value = serde_json::from_str(&job_payload).expect("payload json");
    assert_eq!(
        parsed["paths"][0],
        serde_json::json!(stale_key),
        "the durable job keeps the object paths"
    );
    assert_eq!(
        parsed["server_location"]["bucket"],
        serde_json::json!("moved-bucket"),
        "the durable job keeps the actual storage identity"
    );

    // Nobody can delete with mismatched credentials in the meantime.
    app.clock.advance(40);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    assert!(
        storage.object(stale_key).is_some(),
        "a waited job never deletes with wrong credentials"
    );

    // The operator restores the bucket identity; the next sweep finishes.
    let mut restored = parsed.clone();
    restored["server_location"]["bucket"] = serde_json::json!("statesu-bucket");
    sqlx::query("UPDATE media_jobs SET payload = ?")
        .bind(restored.to_string())
        .execute(&app.app.pool)
        .await
        .expect("config restored");
    app.clock.advance(2000);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    assert!(
        storage.object(stale_key).is_none(),
        "the restored config completes the single deletion"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media WHERE id = ?")
        .bind(row.to_string())
        .fetch_one(&app.app.pool)
        .await
        .expect("row count");
    assert_eq!(rows, 0, "the record is gone once the objects are");
}

/// Configuration waits escalate their backoff (30 s → 1 h) even though
/// they never burn retry attempts; sparse sweeps cannot pin the wait at
/// its base delay.
#[tokio::test]
async fn configuration_waits_escalate_backoff_without_spending_retries() {
    use statesu::clock::Clock;
    let (app, _storage) = TestApp::with_server_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let token = common::create_pat(&app, &cookie, &csrf, true).await;
    let blog_id = create_blog(&app, &token, "Backoff", "media-wait-backoff")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let me = app
        .send(session_authorized(
            common::empty(Method::GET, "/auth/me"),
            &cookie,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let owner_uuid = uuid::Uuid::parse_str(me.json()["id"].as_str().unwrap()).expect("owner uuid");
    let blog_uuid = uuid::Uuid::parse_str(&blog_id).expect("blog uuid");
    let row = uuid::Uuid::now_v7();
    let location = serde_json::json!({
        "account_id": "abcdef0123456789abcdef0123456789",
        "bucket": "moved-bucket",
        "prefix": "moved",
        "public_base_url": "https://media.test.example",
    })
    .to_string()
    .into_bytes();
    sqlx::query(
        "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, \
         storage_location, status, mime, stored_bytes, reserved_bytes, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, 'https://media.test.example', ?, 'ready', 'image/png', 12, 0, ?, ?)",
    )
    .bind(row.to_string())
    .bind(blog_uuid.to_string())
    .bind(owner_uuid.to_string())
    .bind(location)
    .bind(common::NOW)
    .bind(common::NOW)
    .execute(&app.app.pool)
    .await
    .expect("backoff media row");
    sqlx::query(
        "INSERT INTO media_variants (media_id, label, object_key, mime, width, height, bytes) \
         VALUES (?, 'source', ?, 'image/png', 20, 20, 12)",
    )
    .bind(row.to_string())
    .bind("moved-bucket/blogs/wait/abc/source.png")
    .execute(&app.app.pool)
    .await
    .expect("backoff variant row");
    let deleted = app
        .send(authed_request(
            Method::DELETE,
            &format!("/blogs/{blog_id}/media/{row}"),
            &token,
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());

    // Three consecutive waits on a still-mismatched config, each observed
    // as soon as it becomes due; attempts never move, the delay does.
    let mut delays: Vec<i64> = Vec::new();
    while delays.len() < 3 {
        app.clock.advance(1);
        app.app
            .state
            .media
            .run_maintenance(app.clock.now())
            .await
            .expect("maintenance");
        let (attempts, next_run, payload): (i64, i64, String) =
            sqlx::query_as("SELECT attempts, next_run_at, payload FROM media_jobs")
                .fetch_one(&app.app.pool)
                .await
                .expect("job after sweep");
        assert_eq!(attempts, 0, "configuration waits never burn attempts");
        let wait_count = serde_json::from_str::<serde_json::Value>(&payload).expect("payload json")
            ["wait_count"]
            .as_i64()
            .unwrap_or(0);
        if wait_count == i64::try_from(delays.len() + 1).unwrap_or(i64::MAX) {
            delays.push(next_run - app.clock.now());
        }
    }
    assert_eq!(delays, vec![30, 60, 120], "waits escalate: {delays:?}");
}

/// Pending cleanup jobs on custom connections adopt rotated credentials.
#[tokio::test]
async fn pending_cleanup_jobs_pick_up_rotated_custom_credentials() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_configured_storage(|config| {
        config.r2_default = Some(TestApp::test_server_storage());
    })
    .await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let blog = session_blog(&app, &cookie, &csrf, "media-rotate-pending").await;
    let created = app
        .send(storage_json(
            Method::POST,
            &format!("/blogs/{blog}/storage-connections"),
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    let id = created.json()["id"].as_str().unwrap().to_owned();
    let update_path = format!("/blogs/{blog}/storage-connections/{id}");

    let rotated = app
        .send(storage_json(
            Method::PATCH,
            &update_path,
            &cookie,
            &csrf,
            json!({ "access_key_id": "rotated-key", "secret_key": "rotated-secret" }),
        ))
        .await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.text());

    app.clock.advance(40);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("cleaning");
    // Custom connection material flows through the current row abreast of
    // rotation; retired keys never reappear in cleanup lookups.
    let keys_used = storage.used_credentials();
    assert!(
        keys_used.is_empty()
            || keys_used
                .iter()
                .all(|entry| entry.ends_with("/rotated-key")),
        "custom connection material never freezes retired keys: {keys_used:?}"
    );
}

/// Deleting a blog on a custom connection must clean with the
/// connection's material — never the server bucket, whose identity the
/// payload records only for bookkeeping.
#[tokio::test]
async fn blog_delete_on_custom_connection_cleans_with_connection_material() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_configured_storage(|config| {
        config.r2_default = Some(TestApp::test_server_storage());
    })
    .await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let blog_id = session_blog(&app, &cookie, &csrf, "media-custom-blog-cleanup").await;
    let token = common::create_pat(&app, &cookie, &csrf, true).await;
    let created_connection = app
        .send(storage_json(
            Method::POST,
            &format!("/blogs/{blog_id}/storage-connections"),
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    assert_eq!(
        created_connection.status,
        StatusCode::CREATED,
        "{}",
        created_connection.text()
    );
    let connection_id = created_connection.json()["id"].as_str().unwrap().to_owned();
    let preference = app
        .send(storage_json(
            Method::PUT,
            &format!("/blogs/{blog_id}/media-storage"),
            &cookie,
            &csrf,
            json!({ "connection_id": connection_id }),
        ))
        .await;
    assert_eq!(preference.status, StatusCode::OK, "{}", preference.text());

    let uploaded = upload_raw(
        &app,
        Method::POST,
        &format!("/blogs/{blog_id}/media"),
        &token,
        media_png_bytes(),
    )
    .await;
    assert_eq!(uploaded.status, StatusCode::CREATED, "{}", uploaded.text());
    let base = "https://media.example.com/";
    let mut keys: Vec<String> = uploaded
        .json()
        .as_object()
        .unwrap()
        .values()
        .filter_map(|value| value.as_str())
        .filter_map(|url| url.strip_prefix(base))
        .map(|key| key.to_owned())
        .collect();
    if let Some(variants) = uploaded.json()["variants"].as_array() {
        for variant in variants {
            if let Some(url) = variant["public_url"].as_str()
                && let Some(key) = url.strip_prefix(base)
            {
                keys.push(key.to_owned());
            }
        }
    }
    keys.sort();
    keys.dedup();
    assert!(!keys.is_empty(), "upload produced object keys: {keys:?}");

    let deleted = app
        .send(authed_request(
            Method::DELETE,
            &format!("/blogs/{blog_id}"),
            &token,
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    app.clock.advance(40);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    for key in &keys {
        assert!(
            storage.object(key).is_none(),
            "custom connection blog cleanup cleans its objects: {key}"
        );
    }
    let used = storage.used_credentials();
    assert!(
        !used.is_empty()
            && used
                .iter()
                .all(|entry| entry.ends_with("/media-bucket/ak-32bytes")),
        "blog cleanup on a custom connection uses the connection's own credentials, not the server bucket: {used:?}"
    );
}

/// Storage that cannot resolve at snapshot time still leaves the object
/// paths durable in a maintenance job: a returning config or row drains
/// it, and no cascade ever erases the cleanup metadata with it.
#[tokio::test]
async fn blog_delete_keeps_durable_job_for_unresolvable_storage() {
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let token = common::create_pat(&app, &cookie, &csrf, true).await;
    let blog_id = create_blog(&app, &token, "Orphan", "media-orphan-job")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let me = app
        .send(session_authorized(
            common::empty(Method::GET, "/auth/me"),
            &cookie,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let owner_uuid = uuid::Uuid::parse_str(me.json()["id"].as_str().unwrap()).expect("owner uuid");
    let blog_uuid = uuid::Uuid::parse_str(&blog_id).expect("blog uuid");

    // A server-row committed while the config pointed elsewhere: its
    // location snapshot names a bucket today's config cannot serve.
    let row = uuid::Uuid::now_v7();
    let stale_key = "moved-bucket/blogs/orphan/abc/source.png";
    let location = serde_json::json!({
        "account_id": "abcdef0123456789abcdef0123456789",
        "bucket": "moved-bucket",
        "prefix": "moved",
        "public_base_url": "https://media.test.example",
    })
    .to_string()
    .into_bytes();
    sqlx::query(
        "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, \
         storage_location, status, mime, stored_bytes, reserved_bytes, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, 'https://media.test.example', ?, 'ready', 'image/png', 12, 0, ?, ?)",
    )
    .bind(row.to_string())
    .bind(blog_uuid.to_string())
    .bind(owner_uuid.to_string())
    .bind(location)
    .bind(common::NOW)
    .bind(common::NOW)
    .execute(&app.app.pool)
    .await
    .expect("orphan media row");
    sqlx::query(
        "INSERT INTO media_variants (media_id, label, object_key, mime, width, height, bytes) \
         VALUES (?, 'source', ?, 'image/png', 20, 20, 12)",
    )
    .bind(row.to_string())
    .bind(stale_key)
    .execute(&app.app.pool)
    .await
    .expect("orphan variant row");
    statesu::storage::StorageFactory::store_for(
        storage.as_ref(),
        &statesu::storage::BucketConnection {
            account_id: "abcdef0123456789abcdef0123456789".to_owned(),
            bucket: "statesu-bucket".to_owned(),
            prefix: "statesu".to_owned(),
            access_key_id: "server-key".to_owned(),
            secret_key: "server-secret".to_owned(),
        },
    )
    .expect("store")
    .put(stale_key, b"orphan object")
    .await
    .expect("seed object");

    // The deletion itself succeeds — the cascade must not drag the
    // cleanup metadata down with it.
    let deleted = app
        .send(authed_request(
            Method::DELETE,
            &format!("/blogs/{blog_id}"),
            &token,
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text());
    let job_payload: String =
        sqlx::query_scalar("SELECT payload FROM media_jobs ORDER BY created_at LIMIT 1")
            .fetch_one(&app.app.pool)
            .await
            .expect("cleanup job row");
    assert!(
        job_payload.contains(stale_key),
        "the durable job keeps the object paths: {job_payload}"
    );
    assert!(
        job_payload.contains("moved-bucket"),
        "the durable job keeps the storage identity for the wait: {job_payload}"
    );

    // Until the identity returns, the job waits without consuming attempts.
    app.clock.advance(40);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    let (attempts, last_error, next_run): (i64, String, i64) =
        sqlx::query_as("SELECT attempts, last_error, next_run_at FROM media_jobs")
            .fetch_one(&app.app.pool)
            .await
            .expect("job state");
    assert!(last_error.contains("server bucket identity changed"));
    assert_eq!(attempts, 0, "configuration waits never burn retry attempts");
    assert!(
        next_run > common::NOW,
        "the job reschedules into the future"
    );
    assert!(
        storage.object(stale_key).is_some(),
        "a waited job never deletes with mismatched credentials"
    );

    // The identity comes back (operator restores the bucket); the very
    // next maintenance sweep finishes the cleanup with current creds.
    // Only the recorded identity changes — the object paths stay as
    // committed, still pointing at the original keys.
    let mut restored: Value = serde_json::from_str(&job_payload).expect("payload json");
    restored["server_location"]["bucket"] = json!("statesu-bucket");
    sqlx::query("UPDATE media_jobs SET payload = ?")
        .bind(restored.to_string())
        .execute(&app.app.pool)
        .await
        .expect("config restored");
    app.clock.advance(4000);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    // No object claim: the object was never written here; the contract
    // is that the job drained instead of stalling forever.
    let jobs_left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_jobs")
        .fetch_one(&app.app.pool)
        .await
        .expect("job count");
    assert_eq!(jobs_left, 0, "a restored config drains the durable job");
    assert!(
        storage.object(stale_key).is_none(),
        "the restored config cleans the object with current credentials"
    );
}

/// An upload that finalizes between the cleanup snapshot and the delete
/// transaction keeps its media id but changes its variant paths; the
/// fingerprint guard must abort with a retryable conflict instead of
/// committing cleanup jobs that address the wrong paths. The retried
/// deletion then succeeds and maintenance cleans the real bucket.
#[tokio::test]
async fn blog_delete_snapshot_catches_uploading_to_ready_race() {
    use statesu::blog::repo::BlogRepository;
    use statesu::clock::Clock;
    let (app, storage) = TestApp::with_server_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let token = common::create_pat(&app, &cookie, &csrf, true).await;
    let blog_id = create_blog(&app, &token, "Race", "media-race-snapshot")
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let me = app
        .send(session_authorized(
            common::empty(Method::GET, "/auth/me"),
            &cookie,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let owner_uuid = uuid::Uuid::parse_str(me.json()["id"].as_str().unwrap()).expect("owner uuid");
    let blog_uuid = uuid::Uuid::parse_str(&blog_id).expect("blog uuid");

    // An upload mid-flight: row committed in `uploading`, no variants yet.
    let row = uuid::Uuid::now_v7();
    let location = serde_json::json!({
        "account_id": "abcdef0123456789abcdef0123456789",
        "bucket": "statesu-bucket",
        "prefix": "statesu",
        "public_base_url": "https://media.test.example",
    })
    .to_string()
    .into_bytes();
    sqlx::query(
        "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, \
         storage_location, status, mime, stored_bytes, reserved_bytes, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, 'https://media.test.example', ?, 'uploading', 'image/png', \
                 0, 0, ?, ?)",
    )
    .bind(row.to_string())
    .bind(blog_uuid.to_string())
    .bind(owner_uuid.to_string())
    .bind(location.clone())
    .bind(common::NOW)
    .bind(common::NOW)
    .execute(&app.app.pool)
    .await
    .expect("uploading row");

    // Snapshot the cleanup work as the delete request would.
    let (fingerprints, jobs) = app
        .app
        .state
        .media
        .collect_blog_cleanup(owner_uuid, blog_uuid)
        .await
        .expect("snapshot");
    let inserts: Vec<statesu::media::media_repo::MediaJobInsert> = jobs
        .iter()
        .map(statesu::media::service::PreparedCleanupJob::to_insert)
        .collect();

    // The upload completes between snapshot and delete: same media id,
    // new variant paths committed to the row, object written out.
    let object_key = format!("statesu/blogs/{blog_uuid}/{row}/source.png");
    statesu::storage::StorageFactory::store_for(
        storage.as_ref(),
        &statesu::storage::BucketConnection {
            account_id: "abcdef0123456789abcdef0123456789".to_owned(),
            bucket: "statesu-bucket".to_owned(),
            prefix: "statesu".to_owned(),
            access_key_id: "server-key".to_owned(),
            secret_key: "server-secret".to_owned(),
        },
    )
    .expect("store")
    .put(&object_key, b"finished upload bytes")
    .await
    .expect("landed object");
    let now = common::NOW;
    sqlx::query(
        "UPDATE media SET status = 'ready', stored_bytes = 100, reserved_bytes = 0, \
         width = 20, height = 20, updated_at = ? WHERE id = ?",
    )
    .bind(now)
    .bind(row.to_string())
    .execute(&app.app.pool)
    .await
    .expect("finalize race row");
    sqlx::query(
        "INSERT INTO media_variants (media_id, label, object_key, mime, width, height, bytes) \
         VALUES (?, 'source', ?, 'image/png', 20, 20, 100)",
    )
    .bind(row.to_string())
    .bind(&object_key)
    .execute(&app.app.pool)
    .await
    .expect("variant row");

    // The stale snapshot must abort the whole delete-with-cleanup.
    let blogs = BlogRepository::new(app.app.pool.clone());
    let stale = blogs
        .delete_with_media_cleanup(&blog_uuid, &owner_uuid, &fingerprints, &inserts)
        .await;
    assert!(
        matches!(stale, Err(statesu::error::Error::RetrySnapshot)),
        "stale snapshot must abort: {stale:?}"
    );
    let still_there: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blogs WHERE id = ?")
        .bind(blog_uuid.to_string())
        .fetch_one(&app.app.pool)
        .await
        .expect("blog count");
    assert_eq!(still_there, 1, "blog survives the aborted transaction");
    let orphan_jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_jobs")
        .fetch_one(&app.app.pool)
        .await
        .expect("job count");
    assert_eq!(
        orphan_jobs, 0,
        "no cleanup job survives a rolled-back delete"
    );

    // Retrying with a fresh snapshot works and reaches the right bucket.
    app.app
        .state
        .media
        .delete_blog_with_cleanup(owner_uuid, blog_uuid)
        .await
        .expect("retry delete");
    app.clock.advance(40);
    app.app
        .state
        .media
        .run_maintenance(app.clock.now())
        .await
        .expect("maintenance");
    assert!(
        storage.object(&object_key).is_none(),
        "blog deletion cleans the real bucket through the retried job"
    );
    let account_gone: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blogs WHERE id = ?")
        .bind(blog_uuid.to_string())
        .fetch_one(&app.app.pool)
        .await
        .expect("blog count");
    assert_eq!(account_gone, 0);
}

// Media storage connections (credential surface; uploads come later)
// -------------------------------------------------------------------------

/// Creates a session-owned blog and returns its id.
async fn session_blog(app: &TestApp, cookie: &str, csrf: &str, slug: &str) -> String {
    let created = app
        .send(storage_json(
            Method::POST,
            "/blogs",
            cookie,
            csrf,
            json!({ "name": "Media", "slug": slug }),
        ))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    created.json()["id"].as_str().unwrap().to_owned()
}

fn connection_body(account: &str) -> Value {
    json!({
        "name": "Media Bucket",
        "account_id": account,
        "bucket": "media-bucket",
        "prefix": "statesu/media",
        "public_base_url": "https://media.example.com",
        "access_key_id": "ak-32bytes",
        "secret_key": "sk-64bytes-long-enough"
    })
}

/// Session-authorized request against one media storage route; GETs too,
/// because cookie auth is the only accepted surface for storage secrets.
fn storage_json(
    method: Method,
    path: &str,
    cookie: &str,
    csrf: &str,
    body: Value,
) -> Request<Body> {
    session_authorized(json_request(method, path, body), cookie, csrf)
}

#[tokio::test]
async fn storage_connections_round_trip_without_ever_returning_secrets() {
    let (app, storage) = TestApp::with_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let blog = session_blog(&app, &cookie, &csrf, "media-test").await;

    // Absent choice reads as the server bucket.
    let preference = app
        .send(storage_json(
            Method::GET,
            &format!("/blogs/{blog}/media-storage"),
            &cookie,
            &csrf,
            Default::default(),
        ))
        .await;
    assert_eq!(preference.status, StatusCode::OK, "{}", preference.text());
    assert_eq!(
        preference.json(),
        json!({ "storage": "statesu", "connection_id": Value::Null })
    );

    // Create: secrets go in, never come back out.
    let created = app
        .send(storage_json(
            Method::POST,
            &format!("/blogs/{blog}/storage-connections"),
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text());
    let body = created.json();
    assert!(body.get("secret_key").is_none());
    assert_eq!(body["account_id"], "abcdef0123456789abcdef0123456789");
    assert_eq!(body["bucket"], "media-bucket");
    assert_eq!(body["prefix"], "statesu/media");
    assert_eq!(body["public_base_url"], "https://media.example.com");
    let connection_id = body["id"].as_str().unwrap().to_owned();
    // Verification ran through the factory seam exactly once.
    {
        let verified = storage.verified.lock().unwrap();
        assert_eq!(
            verified.last().map(|raw| raw.as_str()),
            Some("abcdef0123456789abcdef0123456789:media-bucket:ak-32bytes")
        );
    }

    // Preference flips to the connection.
    let preference = app
        .send(storage_json(
            Method::PUT,
            &format!("/blogs/{blog}/media-storage"),
            &cookie,
            &csrf,
            json!({ "connection_id": connection_id }),
        ))
        .await;
    assert_eq!(preference.status, StatusCode::OK, "{}", preference.text());
    assert_eq!(preference.json()["storage"], "custom");
    assert_eq!(preference.json()["connection_id"], connection_id);

    // Listing keeps the same shape.
    let list = app
        .send(storage_json(
            Method::GET,
            &format!("/blogs/{blog}/storage-connections"),
            &cookie,
            &csrf,
            Default::default(),
        ))
        .await;
    assert_eq!(list.json().as_array().unwrap().len(), 1);
    assert!(list.json()[0].get("secret_key").is_none());

    // Back to the server bucket via null.
    let preference = app
        .send(storage_json(
            Method::PUT,
            &format!("/blogs/{blog}/media-storage"),
            &cookie,
            &csrf,
            json!({ "connection_id": Value::Null }),
        ))
        .await;
    assert_eq!(preference.json()["storage"], "statesu");

    let deleted = app
        .send(storage_json(
            Method::DELETE,
            &format!("/blogs/{blog}/storage-connections/{connection_id}"),
            &cookie,
            &csrf,
            Default::default(),
        ))
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    app.send(storage_json(
        Method::DELETE,
        &format!("/blogs/{blog}/storage-connections/{connection_id}"),
        &cookie,
        &csrf,
        Default::default(),
    ))
    .await
    .assert_error(StatusCode::NOT_FOUND, "blog not found");
}

#[tokio::test]
async fn storage_connection_field_rules() {
    let (app, _storage) = TestApp::with_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let blog = session_blog(&app, &cookie, &csrf, "media-rules").await;
    let create_path = format!("/blogs/{blog}/storage-connections");
    let base = json!({
        "account_id": "ABCDEF0123456789ABCDEF0123456789",
        "bucket": "media-bucket",
        "public_base_url": "https://x.example",
        "access_key_id": "ak",
        "secret_key": "sk"
    });
    for bad in [
        json!({ "account_id": "short" }),
        json!({ "prefix": "a/../b" }),
        json!({ "bucket": "The-Bucket" }),
        json!({ "public_base_url": "http://x.example" }),
        json!({ "access_key_id": "" }),
    ] {
        let mut body = base.clone();
        for (key, value) in bad.as_object().unwrap() {
            body[key.as_str()] = value.clone();
        }
        let response = app
            .send(storage_json(
                Method::POST,
                &create_path,
                &cookie,
                &csrf,
                body,
            ))
            .await;
        assert_eq!(
            response.status,
            StatusCode::BAD_REQUEST,
            "{}",
            response.text()
        );
        assert_eq!(response.json()["error"], "invalid input");
    }
    // Missing credentials are body faults, never silent defaults.
    let mut missing_secret = base.clone();
    missing_secret.as_object_mut().unwrap().remove("secret_key");
    let response = app
        .send(storage_json(
            Method::POST,
            &create_path,
            &cookie,
            &csrf,
            missing_secret,
        ))
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    assert_eq!(response.json()["error"], "invalid body");

    // Unknown fields are rejected outright.
    let response = app
        .send(storage_json(
            Method::POST,
            &create_path,
            &cookie,
            &csrf,
            json!({
                "account_id": "ABCDEF0123456789ABCDEF0123456789",
                "bucket": "media-bucket",
                "public_base_url": "https://x.example",
                "access_key_id": "ak",
                "secret_key": "sk",
                "region": "auto"
            }),
        ))
        .await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn storage_connection_rotation_and_caps() {
    let (app, _storage) = TestApp::with_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let blog = session_blog(&app, &cookie, &csrf, "media-cap").await;
    let create_path = format!("/blogs/{blog}/storage-connections");

    let created = app
        .send(storage_json(
            Method::POST,
            &create_path,
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    let id = created.json()["id"].as_str().unwrap().to_owned();
    let update_path = format!("/blogs/{blog}/storage-connections/{id}");

    // Rotation accepts only complete pairs.
    let half = app
        .send(storage_json(
            Method::PATCH,
            &update_path,
            &cookie,
            &csrf,
            json!({ "access_key_id": "ak2" }),
        ))
        .await;
    assert_eq!(half.status, StatusCode::BAD_REQUEST);
    let rotated = app
        .send(storage_json(
            Method::PATCH,
            &update_path,
            &cookie,
            &csrf,
            json!({ "access_key_id": "ak2", "secret_key": "sk2" }),
        ))
        .await;
    assert_eq!(rotated.status, StatusCode::OK, "{}", rotated.text());
    assert!(rotated.json().get("secret_key").is_none());
    // Rotation verified the *new* credentials, not the retired pair.
    {
        let verified = _storage.verified.lock().unwrap();
        assert_eq!(
            verified.last().map(String::as_str),
            Some("abcdef0123456789abcdef0123456789:media-bucket:ak2")
        );
    }
    let renamed = app
        .send(storage_json(
            Method::PATCH,
            &update_path,
            &cookie,
            &csrf,
            json!({ "name": "Renamed" }),
        ))
        .await;
    assert_eq!(renamed.json()["name"], "Renamed");

    for index in 0..7 {
        let mut body = connection_body("ABCDEF0123456789ABCDEF0123456789");
        body["bucket"] = json!(format!("media-bucket-{index}"));
        let response = app
            .send(storage_json(
                Method::POST,
                &create_path,
                &cookie,
                &csrf,
                body,
            ))
            .await;
        assert_eq!(response.status, StatusCode::CREATED, "{}", response.text());
    }
    let ninth = app
        .send(storage_json(
            Method::POST,
            &create_path,
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    ninth.assert_error(StatusCode::FORBIDDEN, "forbidden");
}

#[tokio::test]
async fn storage_connection_media_reference_blocks_deletion() {
    let (app, _storage) = TestApp::with_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let blog = session_blog(&app, &cookie, &csrf, "media-lock").await;
    let created = app
        .send(storage_json(
            Method::POST,
            &format!("/blogs/{blog}/storage-connections"),
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    let id = created.json()["id"].as_str().unwrap().to_owned();

    // Owner id via the session surface; only test fixtures reach into SQL.
    let me = app
        .send(session_authorized(
            common::empty(Method::GET, "/auth/me"),
            &cookie,
            &csrf,
        ))
        .await;
    assert_eq!(me.status, StatusCode::OK);
    let owner_id = me.json()["id"].as_str().unwrap().to_owned();

    // A committed media row simulates real usage of the connection.
    sqlx::query(
        "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, status, mime, \
         stored_bytes, reserved_bytes, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'https://media.test.example', 'ready', 'image/png', 10, 0, ?, ?)",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(blog.clone())
    .bind(owner_id.clone())
    .bind(id.clone())
    .bind(common::NOW)
    .bind(common::NOW)
    .execute(&app.app.pool)
    .await
    .expect("media row");

    app.send(storage_json(
        Method::DELETE,
        &format!("/blogs/{blog}/storage-connections/{id}"),
        &cookie,
        &csrf,
        Default::default(),
    ))
    .await
    .assert_error(StatusCode::CONFLICT, "storage connection in use");
}

#[tokio::test]
async fn storage_connection_ownership_is_strict() {
    let (app, _storage) = TestApp::with_storage().await;
    let (cookie, csrf) = setup_session(&app, common::EMAIL).await;
    let (other_cookie, other_csrf) = setup_session(&app, "other@example.com").await;
    let blog = session_blog(&app, &cookie, &csrf, "media-owner").await;

    let created = foreign_connection_creation(&app, &other_cookie, &other_csrf, &blog).await;
    created.assert_error(StatusCode::NOT_FOUND, "blog not found");

    let created = app
        .send(storage_json(
            Method::POST,
            &format!("/blogs/{blog}/storage-connections"),
            &cookie,
            &csrf,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let id = created.json()["id"].as_str().unwrap().to_owned();
    let foreign_delete = app
        .send(storage_json(
            Method::DELETE,
            &format!("/blogs/{blog}/storage-connections/{id}"),
            &other_cookie,
            &other_csrf,
            Default::default(),
        ))
        .await;
    foreign_delete.assert_error(StatusCode::NOT_FOUND, "blog not found");
    // A foreign connection id can never become the blog's preference.
    app.send(storage_json(
        Method::GET,
        &format!("/blogs/{blog}/media-storage"),
        &cookie,
        &csrf,
        Default::default(),
    ))
    .await
    .status_check(StatusCode::OK);
}

/// Sends a connection create from the wrong session user.
async fn foreign_connection_creation(
    app: &TestApp,
    other_cookie: &str,
    other_csrf: &str,
    blog: &str,
) -> TestResponse {
    app.send(storage_json(
        Method::POST,
        &format!("/blogs/{blog}/storage-connections"),
        other_cookie,
        other_csrf,
        connection_body("ABCDEF0123456789ABCDEF0123456789"),
    ))
    .await
}

#[tokio::test]
async fn storage_connections_reject_personal_access_tokens() {
    let (app, _storage) = TestApp::with_storage().await;
    // A second user with a session and a write-scoped PAT.
    let (cookie, csrf) = setup_session(&app, "pat@example.com").await;
    let pat = create_pat(&app, &cookie, &csrf, true).await;
    let (cookie2, csrf2) = setup_session(&app, "other2@example.com").await;
    let blog = session_blog(&app, &cookie2, &csrf2, "media-pat").await;
    let _ = (cookie, csrf);

    // Storage secret routes never answer PATs, whatever the scopes.
    let response = app
        .send(authed_request(
            Method::GET,
            &format!("/blogs/{blog}/storage-connections"),
            &pat,
        ))
        .await;
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{}",
        response.text()
    );
    let create = app
        .send(authed_json(
            Method::POST,
            &format!("/blogs/{blog}/storage-connections"),
            &pat,
            connection_body("ABCDEF0123456789ABCDEF0123456789"),
        ))
        .await;
    assert_eq!(create.status, StatusCode::FORBIDDEN, "{}", create.text());
}
