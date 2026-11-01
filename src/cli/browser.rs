//! Loopback browser authorization with S256 PKCE and a per-login state nonce.
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use base64::Engine;
use tokio::sync::{Mutex, oneshot};

use super::{CliError, client};
use crate::crypto::token;

#[derive(Clone)]
struct Callback {
    state: String,
    sender: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

#[derive(serde::Deserialize)]
struct Parameters {
    state: String,
    code: String,
}

async fn callback(
    State(callback): State<Callback>,
    Query(params): Query<Parameters>,
) -> (StatusCode, Html<&'static str>) {
    if token::hash(&params.state) != token::hash(&callback.state)
        || !token::well_formed(&params.code, "su_cli_")
    {
        return (
            StatusCode::BAD_REQUEST,
            Html("Invalid authorization response. Return to your terminal and try again."),
        );
    }
    let Some(sender) = callback.sender.lock().await.take() else {
        return (
            StatusCode::BAD_REQUEST,
            Html("This authorization has already completed."),
        );
    };
    let _ = sender.send(params.code);
    (
        StatusCode::OK,
        Html(
            "<!doctype html><meta name=referrer content=no-referrer><title>Statesu CLI</title><h1>Authorization received</h1><p>Return to your terminal to finish signing in. You can close this tab.</p>",
        ),
    )
}

pub async fn login(api: &client::Api, server: &str) -> Result<client::Pat, CliError> {
    let verifier = token::generate("").map_err(|e| CliError::Config(e.to_string()))?;
    let state = token::generate("").map_err(|e| CliError::Config(e.to_string()))?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token::hash(&verifier));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!("http://{}/callback", listener.local_addr()?);
    let (sender, receiver) = oneshot::channel();
    let app = axum::Router::new()
        .route("/callback", axum::routing::get(callback))
        .layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response
                    .headers_mut()
                    .insert("cache-control", "no-store".parse().unwrap());
                response
                    .headers_mut()
                    .insert("referrer-policy", "no-referrer".parse().unwrap());
                response.headers_mut().insert(
                    "content-security-policy",
                    "default-src 'none'; frame-ancestors 'none'"
                        .parse()
                        .unwrap(),
                );
                response
            },
        ))
        .with_state(Callback {
            state: state.clone(),
            sender: Arc::new(Mutex::new(Some(sender))),
        });
    let mut url = reqwest::Url::parse(server.trim().trim_end_matches('/'))
        .map_err(|_| CliError::usage("invalid server URL"))?;
    url.set_path("/cli/authorize");
    url.query_pairs_mut()
        .append_pair("redirect_uri", &redirect)
        .append_pair("code_challenge", &challenge)
        .append_pair("state", &state);
    let (shutdown, stop) = oneshot::channel();
    let mut task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stop.await;
            })
            .await
    });
    eprintln!(
        "Approve CLI access in your browser. If it does not open, visit on this computer:\n{url}"
    );
    open_browser(url.as_str());
    let result = tokio::time::timeout(Duration::from_secs(300), receiver).await;
    // Let the browser receive its response before releasing the listener.
    let _ = shutdown.send(());
    if tokio::time::timeout(Duration::from_secs(1), &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
    let code = result
        .map_err(|_| {
            CliError::Config("browser login timed out; run `stscli login` to retry".into())
        })?
        .map_err(|_| CliError::Config("browser login callback closed".into()))?;
    api.exchange_cli_code(&code, &verifier, &redirect).await
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    if let Ok(mut child) = command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        // Reap the launcher without blocking the callback listener.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}
