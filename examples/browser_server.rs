//! Playwright-only server: the real router and built assets, a temporary DB,
//! and the existing recording mailer. No test routes enter the production binary.
#[path = "../tests/common/mod.rs"]
mod common;

use std::sync::Arc;

use axum::Json;
use axum::extract::Query;
use axum::routing::get;
use serde::Deserialize;
use serde_json::json;
use statesu::bootstrap::build_app_with_mailer;
use statesu::clock::SystemClock;
use statesu::config::PublicUrl;
use statesu::http::static_files::EmbeddedDocs;

#[derive(Deserialize)]
struct MailQuery {
    email: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("STATESU_BROWSER_API_PORT")
        .unwrap_or_else(|_| "4180".to_owned())
        .parse()?;
    let temp = tempfile::tempdir()?;
    let mut config = common::test_config(temp.path().join("browser.db"));
    config.addr = format!("127.0.0.1:{port}");
    config.public_url = PublicUrl::parse(&format!("http://localhost:{port}"))?;
    config.allowed_origins = vec![config.public_url.origin.clone()];
    config.trusted_proxies.clear();
    let (mailer, _) = common::TestMailer::shared();
    let mailbox = mailer.clone();
    let app = build_app_with_mailer(
        config,
        Arc::new(SystemClock),
        Arc::new(EmbeddedDocs),
        Arc::new(mailer),
    )
    .await?;
    let router = app
        .router
        .clone()
        .route("/__test/ready", get(|| async { "ready" }))
        .route(
            "/__test/mail",
            get(move |Query(query): Query<MailQuery>| {
                let mailbox = mailbox.clone();
                async move { Json(json!({ "links": mailbox.links_for(&query.email) })) }
            }),
        );
    let listener = tokio::net::TcpListener::bind(&app.state.config.addr).await?;
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    app.close_database().await;
    Ok(())
}
