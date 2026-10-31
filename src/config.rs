use std::net::IpAddr;
use std::time::Duration;

use crate::error::Error;

pub const ADDR_ENV: &str = "STATESU_ADDR";
pub const DB_ENV: &str = "STATESU_DB";
pub const EMAIL_KEY_ENV: &str = "STATESU_EMAIL_KEY";
pub const EMAIL_HMAC_KEY_ENV: &str = "STATESU_EMAIL_HMAC_KEY";
pub const JWT_SECRET_ENV: &str = "STATESU_JWT_SECRET";
pub const JWT_AUDIENCE_ENV: &str = "STATESU_JWT_AUDIENCE";
pub const ALLOWED_ORIGINS_ENV: &str = "STATESU_ALLOWED_ORIGINS";
pub const TRUSTED_PROXIES_ENV: &str = "STATESU_TRUSTED_PROXIES";
pub const RATE_LIMIT_RPM_ENV: &str = "STATESU_RATE_LIMIT_RPM";
pub const AUTH_RATE_LIMIT_RPM_ENV: &str = "STATESU_AUTH_RATE_LIMIT_RPM";
pub const MAX_CONCURRENCY_ENV: &str = "STATESU_MAX_CONCURRENCY";
pub const ARGON2_CONCURRENCY_ENV: &str = "STATESU_ARGON2_CONCURRENCY";
pub const LOG_JSON_ENV: &str = "STATESU_LOG_JSON";

const DEFAULT_ADDR: &str = ":8080";
const DEFAULT_DB: &str = "db/statesu.db";
const DEFAULT_AUDIENCE: &str = "statesu-api";
const DEFAULT_RATE_LIMIT_RPM: u64 = 120;
const DEFAULT_AUTH_RATE_LIMIT_RPM: u64 = 10;
const DEFAULT_MAX_CONCURRENCY: usize = 128;
const DEFAULT_ARGON2_CONCURRENCY: usize = 2;

#[derive(Clone)]
pub struct Config {
    /// TCP bind address with a leading `:` normalized to `0.0.0.0:port`.
    pub addr: String,
    pub db_path: String,
    /// AES-256-GCM key for email encryption; exactly 32 bytes.
    pub email_key: [u8; 32],
    pub email_hmac_key: Vec<u8>,
    pub jwt_secret: Vec<u8>,
    pub jwt_audience: String,
    pub allowed_origins: Vec<String>,
    pub trusted_proxies: Vec<IpNet>,
    pub rate_limit_rpm: u64,
    pub auth_rate_limit_rpm: u64,
    pub max_concurrency: usize,
    pub argon2_concurrency: usize,
    pub log_json: bool,
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
    pub fn from_env() -> Result<Config, Error> {
        let addr =
            normalize_addr(&env_no_empty(ADDR_ENV)?.unwrap_or_else(|| DEFAULT_ADDR.to_owned()))?;
        let db_path = env_no_empty(DB_ENV)?.unwrap_or_else(|| DEFAULT_DB.to_owned());
        let email_key = decode_exact_32(&env_required(EMAIL_KEY_ENV)?)?;
        let email_hmac_key =
            decode_at_least_32(&env_required(EMAIL_HMAC_KEY_ENV)?, EMAIL_HMAC_KEY_ENV)?;
        let jwt_secret = decode_at_least_32(&env_required(JWT_SECRET_ENV)?, JWT_SECRET_ENV)?;
        let jwt_audience =
            env_no_empty(JWT_AUDIENCE_ENV)?.unwrap_or_else(|| DEFAULT_AUDIENCE.to_owned());
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
            jwt_secret,
            jwt_audience,
            allowed_origins,
            trusted_proxies,
            rate_limit_rpm,
            auth_rate_limit_rpm,
            max_concurrency,
            argon2_concurrency,
            log_json,
        })
    }
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

fn decode_exact_32(input: &str) -> Result<[u8; 32], Error> {
    let decoded = decode_key(input)?;
    if decoded.len() != 32 {
        return Err(Error::Config(format!(
            "{EMAIL_KEY_ENV} must decode to exactly 32 bytes"
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
        assert!(decode_exact_32(&short).is_err());
        let good = base64::engine::general_purpose::STANDARD.encode([7u8; 32]);
        assert!(decode_exact_32(&good).is_ok());
        assert!(decode_key("!!!").is_err());
    }
}
