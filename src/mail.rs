use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, InternalError};

/// Delivery seam: production sends through Resend, tests record messages.
/// Hand-rolled boxed futures keep the trait object-safe without a macro dep.
pub type MailFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

pub trait Mailer: Send + Sync {
    /// Sends the account verification mail. `link` is the full absolute URL
    /// the recipient opens to confirm the address.
    fn send_verification<'a>(&'a self, to: &'a str, link: &'a str) -> MailFuture<'a>;
}

/// Resend HTTP API client. Failures map to `Internal` (logged, never
/// returned to callers, who see generic responses either way).
pub struct ResendMailer {
    api_key: Arc<str>,
    from: Arc<str>,
    http: reqwest::Client,
}

/// One attempt, hard-bounded so registration never waits long on mail.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const SEND_ENDPOINT: &str = "https://api.resend.com/emails";

impl ResendMailer {
    pub fn new(api_key: String, from: String) -> Result<ResendMailer, Error> {
        let http = reqwest::Client::builder()
            .timeout(SEND_TIMEOUT)
            .build()
            .map_err(|e| Error::Config(format!("mail client failed to initialize: {e}")))?;
        Ok(ResendMailer {
            api_key: Arc::from(api_key.into_boxed_str()),
            from: Arc::from(from.into_boxed_str()),
            http,
        })
    }
}

impl Mailer for ResendMailer {
    fn send_verification<'a>(&'a self, to: &'a str, link: &'a str) -> MailFuture<'a> {
        Box::pin(async move {
            let response = self
                .http
                .post(SEND_ENDPOINT)
                .bearer_auth(&*self.api_key)
                .json(&serde_json::json!({
                    "from": &*self.from,
                    "to": [to],
                    "subject": SUBJECT,
                    "html": verification_html(link),
                    "text": verification_text(link),
                }))
                .send()
                .await
                .map_err(|e| {
                    Error::Internal(InternalError::new(std::io::Error::other(format!(
                        "resend request failed: {e}"
                    ))))
                })?;
            // Response bodies are logged without echo: they may contain
            // request-identifying details nobody needs.
            if !response.status().is_success() {
                return Err(Error::Internal(InternalError::new(std::io::Error::other(
                    format!("resend rejected mail with status {}", response.status()),
                ))));
            }
            Ok(())
        })
    }
}

const SUBJECT: &str = "Verify your Statesu email";
const EXPIRY_MINUTES: u64 = 30;

/// Monochrome verification mail: white background, black text, inline styles
/// only, no remote assets. Table layout for maximum client compatibility.
pub fn verification_html(link: &str) -> String {
    let mut link_html = String::with_capacity(link.len());
    // The link is server-built and can contain no HTML metacharacters, but
    // rendering goes through escaping so the guarantee is structural.
    escape(link, &mut link_html);
    format!(
        r#"<!doctype html>
<html>
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>
<body style="margin:0;padding:0;background:#ffffff;color:#171717;font-family:Helvetica,Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:#ffffff;">
  <tr><td style="padding:32px 16px;">
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:480px;margin:0 auto;border:1px solid #e5e5e5;">
      <tr><td style="padding:24px 32px;border-bottom:1px solid #e5e5e5;">
        <span style="font-size:14px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:#171717;">Statesu</span>
      </td></tr>
      <tr><td style="padding:32px;">
        <h1 style="margin:0 0 16px;font-size:20px;line-height:1.3;color:#171717;">Verify your email</h1>
        <p style="margin:0 0 24px;font-size:15px;line-height:1.6;color:#171717;">
          Someone registered a Statesu account with this address. Confirm it
          once and the account is ready. The link below expires in
          {EXPIRY_MINUTES} minutes and works one time.
        </p>
        <table role="presentation" cellpadding="0" cellspacing="0" border="0">
          <tr><td style="background:#171717;border-radius:4px;">
            <a href="{link_html}" style="display:inline-block;padding:12px 28px;font-size:15px;color:#ffffff;text-decoration:none;">Verify email</a>
          </td></tr>
        </table>
        <p style="margin:24px 0 0;font-size:13px;line-height:1.6;color:#525252;">
          If the button does nothing, open this address in your browser:
        </p>
        <p style="margin:8px 0 0;font-size:13px;line-height:1.6;">
          <span style="font-family:Menlo,Consolas,monospace;color:#171717;word-break:break-all;">{link_html}</span>
        </p>
      </td></tr>
      <tr><td style="padding:20px 32px;border-top:1px solid #e5e5e5;">
        <p style="margin:0;font-size:12px;line-height:1.6;color:#737373;">
          Did not register? Ignore this mail — the address stays unverified and
          nothing else happens.
        </p>
      </td></tr>
    </table>
  </td></tr>
</table>
</body>
</html>
"#
    )
}

/// Plain-text alternative carrying the same contract.
pub fn verification_text(link: &str) -> String {
    format!(
        "Statesu — verify your email\n\n\
         Someone registered a Statesu account with this address. Confirm it once \
         and the account is ready. The link expires in {EXPIRY_MINUTES} minutes \
         and works one time.\n\n{link}\n\n\
         Did not register? Ignore this mail; the address stays unverified.\n"
    )
}

/// Minimal HTML escaping for attribute/text interpolation.
fn escape(input: &str, out: &mut String) {
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_embeds_the_link_escaped_and_monochrome() {
        let html = verification_html("https://security.statesu.com/verify?token=sv_ab&amp;.1");
        // Metacharacters are escaped, the token itself stays intact.
        assert!(
            html.contains("https://security.statesu.com/verify?token=sv_ab&amp;amp;.1"),
            "{}",
            html
        );
        assert!(!html.contains("<script"));
        assert!(html.contains("background:#ffffff"));
        assert!(html.contains("font-family:Helvetica,Arial,sans-serif;"));
    }

    #[test]
    fn text_contains_the_link_plain() {
        let text = verification_text("https://example.com/verify?token=sv_x.1");
        assert!(text.contains("https://example.com/verify?token=sv_x.1"));
    }

    #[test]
    fn default_expiry_copy_mentions_minutes() {
        assert!(verification_html("https://a/verify").contains("30 minutes"));
        assert!(verification_text("https://a/verify").contains("30 minutes"));
    }
}
