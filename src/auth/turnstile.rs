use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, InternalError};

/// Human-verification seam: production checks with Cloudflare Turnstile,
/// tests substitute a stub. Same hand-rolled boxed-future pattern as `Mailer`.
pub type TurnstileFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

pub trait TurnstileVerifier: Send + Sync {
    /// Confirms a browser-solved Turnstile token for this deployment.
    /// Any rejection — missing, replayed, stale, wrong site — maps to a
    /// uniform failure, never to Cloudflare's raw error codes.
    fn verify<'a>(&'a self, token: &'a str, remote_ip: Option<&'a str>) -> TurnstileFuture<'a>;
}

/// Cloudflare siteverify client. The secret key stays server-side; the token
/// is accepted exactly once by Cloudflare, so replays fail closed by itself.
pub struct CloudflareTurnstile {
    secret: Arc<str>,
    http: reqwest::Client,
}

/// One attempt, hard-bounded so login and register stay responsive.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(5);
const VERIFY_ENDPOINT: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

impl CloudflareTurnstile {
    pub fn new(secret: String) -> Result<CloudflareTurnstile, Error> {
        let http = reqwest::Client::builder()
            .timeout(VERIFY_TIMEOUT)
            .build()
            .map_err(|e| Error::Config(format!("turnstile client failed to initialize: {e}")))?;
        Ok(CloudflareTurnstile {
            secret: Arc::from(secret.into_boxed_str()),
            http,
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct SiteverifyResponse {
    success: bool,
}

impl TurnstileVerifier for CloudflareTurnstile {
    fn verify<'a>(&'a self, token: &'a str, remote_ip: Option<&'a str>) -> TurnstileFuture<'a> {
        Box::pin(async move {
            let mut form = vec![
                ("secret", self.secret.to_string()),
                ("response", token.to_string()),
            ];
            if let Some(ip) = remote_ip {
                form.push(("remoteip", ip.to_string()));
            }
            let response = self
                .http
                .post(VERIFY_ENDPOINT)
                .form(&form)
                .send()
                .await
                .map_err(|e| {
                    Error::Internal(InternalError::new(std::io::Error::other(format!(
                        "turnstile siteverify request failed: {e}"
                    ))))
                })?;
            if !response.status().is_success() {
                // Anything other than a siteverify answer is infrastructure
                // trouble, logged internally; the caller only sees a rejection.
                let status = response.status();
                return Err(Error::Internal(InternalError::new(std::io::Error::other(
                    format!("turnstile siteverify returned status {status}"),
                ))));
            }
            let verdict: SiteverifyResponse = response.json().await.map_err(|e| {
                Error::Internal(InternalError::new(std::io::Error::other(format!(
                    "turnstile siteverify body was invalid: {e}"
                ))))
            })?;
            if verdict.success {
                Ok(())
            } else {
                Err(Error::TurnstileFailed)
            }
        })
    }
}
