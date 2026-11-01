//! HTTP client for the Statesu management API: bearer PATs on `/blogs`,
//! browser authorization codes for the login bootstrap. Redirects are never
//! followed, so a saved token can never leak through a Location hop.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::cli::error::CliError;

/// Page size used when scanning listings to resolve a slug locally.
pub(crate) const MAX_PAGE: u32 = 100;

// Wire shapes ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blog {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub content_markdown: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total: i64,
}

/// A freshly created token as the server reports it; `secret` is shown once.
#[derive(Debug, Clone, Deserialize)]
pub struct Pat {
    pub id: String,
    pub secret: String,
}

/// Sparse patch body: `None` fields stay unchanged server-side.
#[derive(Debug, Default, Serialize)]
pub struct Patch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateBlogBody<'a> {
    name: &'a str,
    slug: &'a str,
}

#[derive(Debug, Serialize)]
struct CreatePostBody<'a> {
    slug: &'a str,
    title: &'a str,
    content_markdown: &'a str,
    status: Option<&'a str>,
}

/// Typed REST client. Only the responses the CLI consumes are typed; extra
/// server fields are ignored by deserialization.
pub struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    /// Builds a client for a base URL: HTTPS, or plain HTTP only for local
    /// development; no path prefix, no trailing slash.
    pub fn new(server: &str) -> Result<Self, CliError> {
        let base = server.trim().trim_end_matches('/');
        let url = reqwest::Url::parse(base).map_err(|_| {
            CliError::usage(format!(
                "invalid server URL '{server}': scheme://host[:port]"
            ))
        })?;
        let usable = url.path() == "/"
            && match url.scheme() {
                "https" => url.host_str().is_some(),
                "http" => is_local(url.host_str()),
                _ => false,
            };
        if !usable {
            return Err(CliError::usage(format!(
                "invalid server URL '{server}': use https://host (plain http only for localhost), without a path"
            )));
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|err| CliError::Config(format!("cannot build HTTP client: {err}")))?;
        Ok(Api {
            http,
            base: base.to_owned(),
        })
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        pat: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let builder = self.http.request(method, format!("{}{path}", self.base));
        match pat {
            Some(pat) => builder.bearer_auth(pat),
            None => builder,
        }
    }

    pub async fn exchange_cli_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<Pat, CliError> {
        self.json_ok(
            self.request(reqwest::Method::POST, "/auth/cli/exchange", None)
                .json(&serde_json::json!({
                    "code": code, "code_verifier": verifier, "redirect_uri": redirect_uri,
                }))
                .send()
                .await?,
        )
        .await
    }

    pub async fn revoke_token(&self, token: &str) -> Result<(), CliError> {
        self.unit_ok(
            self.request(reqwest::Method::POST, "/auth/cli/revoke", None)
                .json(&serde_json::json!({ "token": token }))
                .send()
                .await?,
        )
        .await
    }

    // Blogs ----------------------------------------------------------------

    pub async fn blogs(&self, pat: &str) -> Result<Vec<Blog>, CliError> {
        self.json_ok(
            self.request(reqwest::Method::GET, "/blogs", Some(pat))
                .send()
                .await?,
        )
        .await
    }

    pub async fn create_blog(&self, pat: &str, name: &str, slug: &str) -> Result<Blog, CliError> {
        self.json_ok(
            self.request(reqwest::Method::POST, "/blogs", Some(pat))
                .json(&CreateBlogBody { name, slug })
                .send()
                .await?,
        )
        .await
    }

    // Posts ----------------------------------------------------------------

    pub async fn posts(
        &self,
        pat: &str,
        blog_id: &str,
        page: u32,
        size: u32,
    ) -> Result<Paged<Post>, CliError> {
        self.json_ok(
            self.request(
                reqwest::Method::GET,
                &format!("/blogs/{blog_id}/posts"),
                Some(pat),
            )
            .query(&[("page", page.to_string()), ("size", size.to_string())])
            .send()
            .await?,
        )
        .await
    }

    pub async fn post(&self, pat: &str, blog_id: &str, post_id: &str) -> Result<Post, CliError> {
        self.json_ok(
            self.request(
                reqwest::Method::GET,
                &format!("/blogs/{blog_id}/posts/{post_id}"),
                Some(pat),
            )
            .send()
            .await?,
        )
        .await
    }

    pub async fn create_post(
        &self,
        pat: &str,
        blog_id: &str,
        slug: &str,
        title: &str,
        content_markdown: &str,
        status: Option<&str>,
    ) -> Result<Post, CliError> {
        self.json_ok(
            self.request(
                reqwest::Method::POST,
                &format!("/blogs/{blog_id}/posts"),
                Some(pat),
            )
            .json(&CreatePostBody {
                slug,
                title,
                content_markdown,
                status,
            })
            .send()
            .await?,
        )
        .await
    }

    pub async fn patch_post(
        &self,
        pat: &str,
        blog_id: &str,
        post_id: &str,
        patch: &Patch,
    ) -> Result<Post, CliError> {
        self.json_ok(
            self.request(
                reqwest::Method::PATCH,
                &format!("/blogs/{blog_id}/posts/{post_id}"),
                Some(pat),
            )
            .json(patch)
            .send()
            .await?,
        )
        .await
    }

    pub async fn delete_post(
        &self,
        pat: &str,
        blog_id: &str,
        post_id: &str,
    ) -> Result<(), CliError> {
        self.unit_ok(
            self.request(
                reqwest::Method::DELETE,
                &format!("/blogs/{blog_id}/posts/{post_id}"),
                Some(pat),
            )
            .send()
            .await?,
        )
        .await
    }

    // Response plumbing ----------------------------------------------------

    async fn json_ok<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, CliError> {
        if !response.status().is_success() {
            return Err(wire_error(response).await);
        }
        response
            .json()
            .await
            .map_err(|err| CliError::Config(format!("cannot decode server response: {err}")))
    }

    async fn unit_ok(&self, response: reqwest::Response) -> Result<(), CliError> {
        if !response.status().is_success() {
            return Err(wire_error(response).await);
        }
        Ok(())
    }
}

/// Maps a failure response to [`CliError::Api`]: the body's `error` field is
/// the stable message contract; non-JSON failures fall back to the status.
async fn wire_error(response: reqwest::Response) -> CliError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|body| body.get("error")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| {
            status
                .canonical_reason()
                .unwrap_or("request failed")
                .to_owned()
        });
    CliError::Api {
        status: status.as_u16(),
        message,
        retry_after,
    }
}

/// Plain HTTP is allowed only for loopback development hosts.
fn is_local(host: Option<&str>) -> bool {
    match host {
        Some(name) if name == "localhost" || name.ends_with(".localhost") => true,
        Some(name) => name
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::Api;

    #[test]
    fn accepts_https_and_local_http_only() {
        assert!(Api::new("https://cms.example.com").is_ok());
        assert!(Api::new("https://cms.example.com/").is_ok());
        assert!(Api::new("http://localhost:8080").is_ok());
        assert!(Api::new("http://127.0.0.1:8080").is_ok());
        assert!(Api::new("http://[::1]:8080").is_ok());

        // Remotely, or with a path prefix, only https is a valid base.
        assert!(Api::new("http://cms.example.com").is_err());
        assert!(Api::new("http://cms.example.com/api").is_err());
        assert!(Api::new("https://cms.example.com/api").is_err());
        assert!(Api::new("ftp://cms.example.com").is_err());
        assert!(Api::new("not a url").is_err());
        assert!(Api::new("").is_err());
    }
}
