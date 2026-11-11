use std::net::IpAddr;
use std::time::Duration;

use crate::error::Error;

pub const ADDR_ENV: &str = "STATESU_ADDR";
pub const DB_ENV: &str = "STATESU_DB";
pub const EMAIL_KEY_ENV: &str = "STATESU_EMAIL_KEY";
pub const EMAIL_HMAC_KEY_ENV: &str = "STATESU_EMAIL_HMAC_KEY";
pub const RESEND_API_KEY_ENV: &str = "STATESU_RESEND";
pub const MAIL_FROM_ENV: &str = "STATESU_MAIL_FROM";
pub const PUBLIC_URL_ENV: &str = "STATESU_PUBLIC_URL";
pub const ALLOWED_ORIGINS_ENV: &str = "STATESU_ALLOWED_ORIGINS";
pub const TRUSTED_PROXIES_ENV: &str = "STATESU_TRUSTED_PROXIES";
pub const RATE_LIMIT_RPM_ENV: &str = "STATESU_RATE_LIMIT_RPM";
pub const AUTH_RATE_LIMIT_RPM_ENV: &str = "STATESU_AUTH_RATE_LIMIT_RPM";
pub const MAX_CONCURRENCY_ENV: &str = "STATESU_MAX_CONCURRENCY";
pub const ARGON2_CONCURRENCY_ENV: &str = "STATESU_ARGON2_CONCURRENCY";
pub const MAX_POSTS_PER_USER_ENV: &str = "STATESU_MAX_POSTS_PER_USER";
pub const MAX_TOTAL_CONTENT_BYTES_ENV: &str = "STATESU_MAX_TOTAL_CONTENT_BYTES";
pub const MEDIA_KEY_ENV: &str = "STATESU_MEDIA_KEY";
pub const R2_ACCOUNT_ID_ENV: &str = "STATESU_R2_ACCOUNT_ID";
pub const R2_BUCKET_ENV: &str = "STATESU_R2_BUCKET";
pub const R2_PREFIX_ENV: &str = "STATESU_R2_PREFIX";
pub const R2_ACCESS_KEY_ID_ENV: &str = "STATESU_R2_ACCESS_KEY_ID";
pub const R2_SECRET_ACCESS_KEY_ENV: &str = "STATESU_R2_SECRET_ACCESS_KEY";
pub const R2_PUBLIC_BASE_URL_ENV: &str = "STATESU_R2_PUBLIC_BASE_URL";
pub const MAX_MEDIA_BYTES_ENV: &str = "STATESU_MAX_MEDIA_BYTES";
pub const MAX_TOTAL_MEDIA_BYTES_ENV: &str = "STATESU_MAX_TOTAL_MEDIA_BYTES";
pub const MAX_MEDIA_PER_USER_ENV: &str = "STATESU_MAX_MEDIA_PER_USER";
pub const LOG_JSON_ENV: &str = "STATESU_LOG_JSON";
pub const TURNSTILE_SECRET_KEY_ENV: &str = "STATESU_TURNSTILE_SECRET_KEY";

const DEFAULT_ADDR: &str = ":8080";
const DEFAULT_DB: &str = "db/statesu.db";
const DEFAULT_RATE_LIMIT_RPM: u64 = 120;
const DEFAULT_AUTH_RATE_LIMIT_RPM: u64 = 10;
const DEFAULT_MAX_CONCURRENCY: usize = 128;
const DEFAULT_ARGON2_CONCURRENCY: usize = 2;
/// Per-user stored-content quotas: oldest content counts too, so these are
/// hard ceilings, not watermarks. Abuse grows linearly with user count.
const DEFAULT_MAX_POSTS_PER_USER: usize = 500;
const DEFAULT_MAX_TOTAL_CONTENT_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_MEDIA_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_MAX_TOTAL_MEDIA_BYTES: usize = 256 * 1024 * 1024;
const DEFAULT_MAX_MEDIA_PER_USER: usize = 1000;

#[derive(Clone)]
pub struct Config {
    /// TCP bind address with a leading `:` normalized to `0.0.0.0:port`.
    pub addr: String,
    pub db_path: String,
    /// AES-256-GCM key for email encryption; exactly 32 bytes.
    pub email_key: [u8; 32],
    pub email_hmac_key: Vec<u8>,
    /// Resend API key for verification mail; server-only secret.
    pub resend_api_key: String,
    /// `From` header value for verification mail, e.g. `Statesu <no-reply@…>`.
    pub mail_from: String,
    /// Public origin the dashboard is served from; verification links and the
    /// same-site Origin check for cookie-authenticated writes use it.
    pub public_url: PublicUrl,
    pub allowed_origins: Vec<String>,
    pub trusted_proxies: Vec<IpNet>,
    pub rate_limit_rpm: u64,
    pub auth_rate_limit_rpm: u64,
    pub max_concurrency: usize,
    pub argon2_concurrency: usize,
    /// Hard cap on posts per user across all of their blogs, including drafts
    /// and soft-deleted rows.
    pub max_posts_per_user: usize,
    /// Hard cap on stored Markdown bytes per user across all of their blogs.
    pub max_total_content_bytes: usize,
    pub log_json: bool,
    /// Cloudflare Turnstile secret key; `None` disables the human check.
    pub turnstile_secret: Option<String>,
    /// AES-256-GCM key decrypting user-supplied R2 access credentials at
    /// rest; exactly 32 bytes. Distinct from the email key.
    pub media_key: [u8; 32],
    /// Server-owned R2 bucket used for new media on blogs without a custom
    /// connection; `None` disables media uploads server-wide.
    pub r2_default: Option<R2Default>,
    /// Hard cap on a single uploaded media file
    /// (`STATESU_MAX_MEDIA_BYTES`).
    pub max_media_bytes: usize,
    /// Hard cap on stored media bytes per user across all blogs
    /// (`STATESU_MAX_TOTAL_MEDIA_BYTES`).
    pub max_total_media_bytes: usize,
    /// Hard cap on media records per user across all blogs
    /// (`STATESU_MAX_MEDIA_PER_USER`).
    pub max_media_per_user: usize,
}

/// The server's own R2 bucket; secrets stay in the environment, never in
/// user-owned rows. `public_base_url` hosts the objects publicly, e.g.
/// `https://media.example.com`.
#[derive(Debug, Clone)]
pub struct R2Default {
    pub account_id: String,
    pub bucket: String,
    /// Key prefix inside the bucket dedicated to Statesu; empty means
    /// bucket root.
    pub prefix: Option<String>,
    pub access_key_id: String,
    pub secret_key: String,
    pub public_base_url: String,
}

/// The public `scheme://host[:port]` origin parsed from `PUBLIC_URL_ENV`.
/// Only the origin is kept: paths, credentials and fragments are rejected so
/// verification links and Origin comparisons cannot be polluted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicUrl {
    /// Scheme plus lowercase authority, e.g. `https://security.statesu.com`.
    pub origin: String,
}

impl PublicUrl {
    pub fn parse(input: &str) -> Result<PublicUrl, Error> {
        let (scheme, rest) = input
            .split_once("://")
            .ok_or_else(|| public_url_error(input))?;
        if !matches!(scheme, "http" | "https") || rest.is_empty() {
            return Err(public_url_error(input));
        }
        // One path/authority segment only: no path, userinfo or fragment.
        if rest
            .split(['/', '?', '#', '@'])
            .next()
            .is_some_and(|authority| authority != rest)
        {
            return Err(public_url_error(input));
        }
        Ok(PublicUrl {
            origin: format!("{scheme}://{rest}").to_lowercase(),
        })
    }

    /// True when an `Origin` header value denotes this origin.
    pub fn matches_origin(&self, origin: &str) -> bool {
        origin.eq_ignore_ascii_case(&self.origin)
    }

    /// Builds an absolute link under this origin.
    pub fn link(&self, path: &str) -> String {
        format!("{}{}", self.origin, path)
    }
}

fn public_url_error(input: &str) -> Error {
    Error::Config(format!(
        "{PUBLIC_URL_ENV} must be a bare http(s) origin, got: {input}"
    ))
}

/// Decoded CIDR network: an integer IP representation plus mask bit count.
/// IPv4 addresses are stored left-shifted into 128 bits so they never match
/// IPv6-CIDR entries and vice versa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpNet {
    pub addr: u128,
    pub prefix_bits: u8,
    pub is_ipv4: bool,
}

impl IpNet {
    pub fn parse(input: &str) -> Result<IpNet, Error> {
        let (addr_part, bits_part) = match input.split_once('/') {
            Some((a, b)) => (a, Some(b)),
            None => (input, None),
        };
        let addr: IpAddr = addr_part.parse().map_err(|_| {
            Error::Config(format!(
                "invalid CIDR address in {TRUSTED_PROXIES_ENV}: {input}"
            ))
        })?;
        let bits: u8 = match bits_part {
            Some(b) => b.parse().map_err(|_| {
                Error::Config(format!(
                    "invalid CIDR prefix in {TRUSTED_PROXIES_ENV}: {input}"
                ))
            })?,
            None => {
                if addr.is_ipv4() {
                    32
                } else {
                    128
                }
            }
        };
        let max_bits = if addr.is_ipv4() { 32 } else { 128 };
        if bits > max_bits {
            return Err(Error::Config(format!(
                "CIDR prefix out of range in {TRUSTED_PROXIES_ENV}: {input}"
            )));
        }
        let shifted = match addr {
            IpAddr::V4(v4) => u128::from(u32::from(v4)) << 96,
            IpAddr::V6(v6) => u128::from(v6),
        };
        Ok(IpNet {
            addr: shifted,
            prefix_bits: bits,
            is_ipv4: addr.is_ipv4(),
        })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let candidate = match ip {
            IpAddr::V4(v4) => u128::from(u32::from(v4)) << 96,
            IpAddr::V6(v6) => u128::from(v6),
        };
        if self.is_ipv4 != ip.is_ipv4() {
            return false;
        }
        let bits = 128 - u32::from(self.prefix_bits);
        (candidate >> bits) == (self.addr >> bits)
    }
}

impl Config {
    /// True when an `Origin` header value is ours for same-site write pinning:
    /// the public URL (the dashboard origin) or any explicitly allowlisted
    /// origin (the dev dashboard through the Vite proxy). Attackers cannot
    /// extend the list, so this stays a server decision.
    pub fn origin_allowed(&self, origin: &str) -> bool {
        self.public_url.matches_origin(origin)
            || self
                .allowed_origins
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(origin))
    }

    pub fn from_env() -> Result<Config, Error> {
        let addr =
            normalize_addr(&env_no_empty(ADDR_ENV)?.unwrap_or_else(|| DEFAULT_ADDR.to_owned()))?;
        let db_path = env_no_empty(DB_ENV)?.unwrap_or_else(|| DEFAULT_DB.to_owned());
        let email_key = decode_exact_32(&env_required(EMAIL_KEY_ENV)?, EMAIL_KEY_ENV)?;
        let email_hmac_key =
            decode_at_least_32(&env_required(EMAIL_HMAC_KEY_ENV)?, EMAIL_HMAC_KEY_ENV)?;
        let resend_api_key = env_required(RESEND_API_KEY_ENV)?;
        let mail_from = env_required(MAIL_FROM_ENV)?;
        let public_url = PublicUrl::parse(&env_required(PUBLIC_URL_ENV)?)?;
        let allowed_origins = match env_no_empty(ALLOWED_ORIGINS_ENV)? {
            Some(v) => v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    http::HeaderValue::try_from(s)
                        .map(|_| s.to_owned())
                        .map_err(|_| {
                            Error::Config(format!(
                                "{ALLOWED_ORIGINS_ENV} contains an invalid origin: {s}"
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let trusted_proxies = match env_no_empty(TRUSTED_PROXIES_ENV)? {
            Some(v) => v
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(IpNet::parse)
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
        };
        let rate_limit_rpm = env_no_empty(RATE_LIMIT_RPM_ENV)?
            .map(|v| parse_positive(&v, RATE_LIMIT_RPM_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_RATE_LIMIT_RPM);
        let auth_rate_limit_rpm = env_no_empty(AUTH_RATE_LIMIT_RPM_ENV)?
            .map(|v| parse_positive(&v, AUTH_RATE_LIMIT_RPM_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_AUTH_RATE_LIMIT_RPM);
        let max_concurrency = env_no_empty(MAX_CONCURRENCY_ENV)?
            .map(|v| parse_usize(&v, MAX_CONCURRENCY_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_CONCURRENCY);
        let argon2_concurrency = env_no_empty(ARGON2_CONCURRENCY_ENV)?
            .map(|v| parse_usize(&v, ARGON2_CONCURRENCY_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_ARGON2_CONCURRENCY);
        let max_posts_per_user = env_no_empty(MAX_POSTS_PER_USER_ENV)?
            .map(|v| parse_usize(&v, MAX_POSTS_PER_USER_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_POSTS_PER_USER);
        let max_total_content_bytes = env_no_empty(MAX_TOTAL_CONTENT_BYTES_ENV)?
            .map(|v| parse_usize(&v, MAX_TOTAL_CONTENT_BYTES_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_TOTAL_CONTENT_BYTES);
        let turnstile_secret = env_no_empty(TURNSTILE_SECRET_KEY_ENV)?;
        let media_key = decode_exact_32(&env_required(MEDIA_KEY_ENV)?, MEDIA_KEY_ENV)?;
        let r2_default = r2_default_from_env()?;
        let max_media_bytes = env_no_empty(MAX_MEDIA_BYTES_ENV)?
            .map(|v| parse_usize(&v, MAX_MEDIA_BYTES_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_MEDIA_BYTES);
        let max_total_media_bytes = env_no_empty(MAX_TOTAL_MEDIA_BYTES_ENV)?
            .map(|v| parse_usize(&v, MAX_TOTAL_MEDIA_BYTES_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_TOTAL_MEDIA_BYTES);
        let max_media_per_user = env_no_empty(MAX_MEDIA_PER_USER_ENV)?
            .map(|v| parse_usize(&v, MAX_MEDIA_PER_USER_ENV))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_MEDIA_PER_USER);
        let log_json = match env_no_empty(LOG_JSON_ENV)? {
            Some(v) => match v.to_ascii_lowercase().as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => {
                    return Err(Error::Config(format!(
                        "{LOG_JSON_ENV} must be true or false"
                    )));
                }
            },
            None => false,
        };

        Ok(Config {
            addr,
            db_path,
            email_key,
            email_hmac_key,
            resend_api_key,
            mail_from,
            public_url,
            allowed_origins,
            trusted_proxies,
            rate_limit_rpm,
            auth_rate_limit_rpm,
            max_concurrency,
            argon2_concurrency,
            max_posts_per_user,
            max_total_content_bytes,
            log_json,
            turnstile_secret,
            media_key,
            r2_default,
            max_media_bytes,
            max_total_media_bytes,
            max_media_per_user,
        })
    }
}

/// The optional server-owned R2 bucket; missing and empty group all components
/// consistently. A partially set group never boots: half a bucket config
/// looks like a working deployment but uploads fail at runtime.
fn r2_default_from_env() -> Result<Option<R2Default>, Error> {
    let parts = [
        (R2_ACCOUNT_ID_ENV, env_no_empty(R2_ACCOUNT_ID_ENV)?),
        (R2_BUCKET_ENV, env_no_empty(R2_BUCKET_ENV)?),
        (R2_PREFIX_ENV, env_no_empty(R2_PREFIX_ENV)?),
        (R2_ACCESS_KEY_ID_ENV, env_no_empty(R2_ACCESS_KEY_ID_ENV)?),
        (
            R2_SECRET_ACCESS_KEY_ENV,
            env_no_empty(R2_SECRET_ACCESS_KEY_ENV)?,
        ),
        (
            R2_PUBLIC_BASE_URL_ENV,
            env_no_empty(R2_PUBLIC_BASE_URL_ENV)?,
        ),
    ];
    let all_set = parts.iter().all(|(_, value)| value.is_some());
    let none_set = parts.iter().all(|(_, value)| value.is_none());
    if none_set {
        return Ok(None);
    }
    if !all_set {
        let missing: Vec<&str> = parts
            .iter()
            .filter(|(_, value)| value.is_none())
            .map(|(name, _)| *name)
            .collect();
        return Err(Error::Config(format!(
            "{} must be set together or all omitted",
            missing.join(", ")
        )));
    }
    let mut part_values = parts.into_iter().map(|(_, value)| value.unwrap());
    let account_id = part_values.next().expect("checked present");
    let bucket = part_values.next().expect("checked present");
    let prefix = part_values.next().expect("checked present");
    let access_key_id = part_values.next().expect("checked present");
    let secret_key = part_values.next().expect("checked present");
    let public_base_url = part_values.next().expect("checked present");
    // The env group shares the same field rules as user-created connections;
    // the validation helpers only answer "invalid", so the config error
    // carries the responsible variable name.
    let account_id = crate::media::validate_account_id(&account_id).map_err(|_| {
        Error::Config(format!(
            "{R2_ACCOUNT_ID_ENV} must be a 32-character hex account id"
        ))
    })?;
    let bucket = crate::media::validate_bucket_name(&bucket)
        .map_err(|_| Error::Config(format!("{R2_BUCKET_ENV} must be a valid R2 bucket name")))?;
    let prefix = crate::media::normalize_prefix(Some(&prefix))
        .map_err(|_| Error::Config(format!("{R2_PREFIX_ENV} must be a valid key prefix")))?;
    let public_base_url = crate::media::validate_public_base_url(&public_base_url)
        .map_err(|_| {
            Error::Config(format!(
                "{R2_PUBLIC_BASE_URL_ENV} must be an https origin (with optional path), no trailing slash"
            ))
        })?;
    Ok(Some(R2Default {
        account_id,
        bucket,
        prefix,
        access_key_id,
        secret_key,
        public_base_url,
    }))
}

fn env_required(name: &str) -> Result<String, Error> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(Error::Config(format!(
            "environment variable {name} is required"
        ))),
    }
}

fn env_no_empty(name: &str) -> Result<Option<String>, Error> {
    match std::env::var(name) {
        Ok(v) if v.trim().is_empty() => Err(Error::Config(format!(
            "environment variable {name} must not be empty"
        ))),
        Ok(v) => Ok(Some(v)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(e) => Err(Error::Config(format!(
            "environment variable {name} is invalid: {e}"
        ))),
    }
}

fn normalize_addr(addr: &str) -> Result<String, Error> {
    let normalized = if let Some(rest) = addr.strip_prefix(':') {
        format!("0.0.0.0:{rest}")
    } else {
        addr.to_owned()
    };
    normalized
        .parse::<std::net::SocketAddr>()
        .map(|_| normalized)
        .map_err(|_| Error::Config(format!("invalid bind address {ADDR_ENV}: {addr}")))
}

fn decode_exact_32(input: &str, env: &str) -> Result<[u8; 32], Error> {
    let decoded = decode_key(input)?;
    if decoded.len() != 32 {
        return Err(Error::Config(format!(
            "{env} must decode to exactly 32 bytes"
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn decode_at_least_32(input: &str, env: &str) -> Result<Vec<u8>, Error> {
    let decoded = decode_key(input)?;
    if decoded.len() < 32 {
        return Err(Error::Config(format!(
            "{env} must decode to at least 32 bytes"
        )));
    }
    Ok(decoded)
}

fn decode_key(input: &str) -> Result<Vec<u8>, Error> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .map_err(|_| Error::Config("key must be valid standard base64".to_owned()))
}

fn parse_positive(v: &str, name: &str) -> Result<u64, Error> {
    let n: u64 = v
        .parse()
        .map_err(|_| Error::Config(format!("{name} must be a positive integer")))?;
    if n == 0 {
        return Err(Error::Config(format!("{name} must be a positive integer")));
    }
    Ok(n)
}

fn parse_usize(v: &str, name: &str) -> Result<usize, Error> {
    let n: usize = v
        .parse()
        .map_err(|_| Error::Config(format!("{name} must be a positive integer")))?;
    if n == 0 {
        return Err(Error::Config(format!("{name} must be a positive integer")));
    }
    Ok(n)
}

/// Pool acquire timeout shared by the SQLite pool config and busy timeout.
pub const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    #[test]
    fn addr_normalization_accepts_leading_colon() {
        assert_eq!(normalize_addr(":8080").unwrap(), "0.0.0.0:8080");
        assert_eq!(normalize_addr("127.0.0.1:9999").unwrap(), "127.0.0.1:9999");
        assert!(normalize_addr("localhost:8080").is_err());
        assert!(normalize_addr(":::").is_err());
    }

    #[test]
    fn cidr_parsing_and_matching() {
        let net = IpNet::parse("10.0.0.0/8").unwrap();
        assert!(net.contains("10.1.2.3".parse().unwrap()));
        assert!(!net.contains("11.1.2.3".parse().unwrap()));

        let single = IpNet::parse("172.16.0.2").unwrap();
        assert!(single.contains("172.16.0.2".parse().unwrap()));

        let v6 = IpNet::parse("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8::1".parse().unwrap()));
        assert!(!v6.contains("10.0.0.1".parse().unwrap()));
        assert!(!net.contains("2001:db8::1".parse().unwrap()));

        assert!(IpNet::parse("10.0.0.0/33").is_err());
        assert!(IpNet::parse("bad/8").is_err());
    }

    #[test]
    fn key_decode_enforcement() {
        let short = base64::engine::general_purpose::STANDARD.encode([7u8; 16]);
        assert!(decode_exact_32(&short, EMAIL_KEY_ENV).is_err());
        let good = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        assert!(decode_exact_32(&good, EMAIL_KEY_ENV).is_ok());
        assert!(decode_key("!!!").is_err());
    }
}
