//! Object storage seam for media. One interface, one R2 implementation.
//!
//! The media service never sees S3 signing details: it asks the
//! [`StorageFactory`] to verify a new connection and for an [`ObjectStore`]
//! when it needs to write or remove objects. Requests are exclusively
//! per-object; the interface deliberately has no bucket listing, so
//! credentials are only ever used for keys Statesu itself created.

use std::future::Future;
use std::net::{IpAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, Mac};
use sha2::Digest;

use crate::clock::Clock;
use crate::error::{Error, InternalError};

/// AWS Signature V4 scope values R2 expects.
const SIGNING_SERVICE: &str = "s3";
const SIGNING_REGION: &str = "auto";
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const SIGNED_HEADERS: &str = "host;x-amz-content-sha256;x-amz-date";

/// Everything needed to reach one bucket. Secrets live in service rows;
/// this type is constructed transiently, never stored or logged.
#[derive(Debug, Clone)]
pub struct BucketConnection {
    pub account_id: String,
    pub bucket: String,
    /// Statesu-dedicated key prefix inside the bucket; empty means the
    /// bucket root.
    pub prefix: String,
    pub access_key_id: String,
    pub secret_key: String,
}

impl BucketConnection {
    /// Callers pass keys relative to the connection prefix.
    pub fn object_path(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}/{}", self.prefix, key)
        }
    }
}

pub type StoreFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

/// Per-object access to one bucket. Implementations are cheap to clone and
/// safe to share across tasks.
pub trait ObjectStore: Send + Sync {
    fn put<'a>(&'a self, key: &'a str, body: &'a [u8]) -> StoreFuture<'a>;
    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a>;
}

/// Storage providers sit behind one seam so neither the media service nor
/// the HTTP layer learns provider endpoints or request signing.
/// Verification exercises the exact path uploads will take: write a probe
/// object, prove the configured public URL serves it byte-for-byte, then
/// remove the probe.
pub trait StorageFactory: Send + Sync {
    fn verify_connection<'a>(
        &'a self,
        connection: &'a BucketConnection,
        public_base_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

    /// Builds a store for runtime object access; performs no network I/O.
    fn store_for(&self, connection: &BucketConnection) -> Result<Arc<dyn ObjectStore>, Error>;
}

/// R2 over S3-compatible request signing. One redirect-less HTTP client:
/// object requests must never bounce off-box.
pub struct R2StorageFactory {
    client: reqwest::Client,
    clock: Arc<dyn Clock>,
}

impl R2StorageFactory {
    pub fn new(clock: Arc<dyn Clock>) -> Result<R2StorageFactory, Error> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(internal)?;
        Ok(R2StorageFactory { client, clock })
    }

    /// Path-style R2 endpoint: the bucket stays in the request path, so
    /// the signed Host is the bare authority and the canonical URI carries
    /// the bucket — one self-consistent signing surface.
    fn endpoint(&self, connection: &BucketConnection) -> String {
        format!("https://{}.r2.cloudflarestorage.com", connection.account_id)
    }
}

impl StorageFactory for R2StorageFactory {
    fn verify_connection<'a>(
        &'a self,
        connection: &'a BucketConnection,
        public_base_url: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            let resolved = ensure_public_host(public_base_url).await?;
            let pinned = pinned_client(public_base_url, &resolved, |builder| {
                builder
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(10))
            })
            .await?;
            let store = self.store_for(connection)?;
            let probe_path = connection.object_path(&format!("probe-{}", uuid::Uuid::now_v7()));
            let probe_body = format!("statesu-verify {}\n", probe_token());

            store.put(&probe_path, probe_body.as_bytes()).await?;
            let public_object =
                format_public_url(public_base_url, &probe_path).ok_or_else(verification_failed)?;
            let served = pinned.get(public_object).send().await;
            let matches = match served {
                Ok(response) if response.status().is_success() => {
                    match tokio::time::timeout(
                        Duration::from_secs(5),
                        read_bounded(response, PROBE_READ_LIMIT),
                    )
                    .await
                    {
                        Ok(Some(bytes)) => bytes == probe_body.as_bytes(),
                        _ => false,
                    }
                }
                _ => false,
            };
            if !matches {
                // Best-effort cleanup; the verification has already failed.
                let _ = store.delete(&probe_path).await;
                tracing::warn!("storage connection does not serve its public URL");
                return Err(verification_failed());
            }
            store.delete(&probe_path).await?;
            Ok(())
        })
    }

    fn store_for(&self, connection: &BucketConnection) -> Result<Arc<dyn ObjectStore>, Error> {
        Ok(Arc::new(R2ObjectStore {
            base_url: self.endpoint(connection),
            bucket: connection.bucket.clone(),
            access_key_id: connection.access_key_id.clone(),
            secret_key: connection.secret_key.clone(),
            client: self.client.clone(),
            clock: self.clock.clone(),
        }))
    }
}

fn internal<E: std::fmt::Display>(err: E) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(err.to_string())))
}

/// Connection-level problems are user-visible, not infrastructure faults.
fn verification_failed() -> Error {
    Error::ConnectionInvalid
}

fn probe_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness");
    hex(&bytes)
}

fn hex(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble"));
        out.push(char::from_digit(u32::from(byte & 0x0F), 16).expect("nibble"));
    }
    out
}

/// Resolves the public URL host before any request leaves the box and
/// refuses addresses that are plausible internal targets. Combined with the
/// redirect-less client, a misconfigured URL cannot be used to bounce
/// requests through the local network.
async fn ensure_public_host(public_base_url: &str) -> Result<Vec<std::net::SocketAddr>, Error> {
    let authority = authority_of(public_base_url).ok_or_else(verification_failed)?;
    let host = authority_host(&authority);
    let port = authority_port(&authority).unwrap_or(443);
    let lookup = format!("{host}:{port}");
    let resolved = tokio::task::spawn_blocking(move || {
        lookup
            .to_socket_addrs()
            .map(|addresses| addresses.collect::<Vec<_>>())
    })
    .await
    .map_err(internal)? // joining, not the resolver result
    .map_err(internal)?;
    if resolved.is_empty() || resolved.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(verification_failed());
    }
    Ok(resolved)
}

/// A client whose DNS for `public_base_url`'s host is pinned to the
/// addresses that were just verified public: the connection target cannot
/// drift to an internal host between check and fetch (DNS rebinding).
async fn pinned_client(
    public_base_url: &str,
    addrs: &[std::net::SocketAddr],
    tune: impl Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder,
) -> Result<reqwest::Client, Error> {
    let authority = authority_of(public_base_url).ok_or_else(verification_failed)?;
    let host = authority_host(&authority);
    let mut builder = tune(reqwest::Client::builder());
    for address in addrs {
        builder = builder.resolve(&host, *address);
    }
    builder.build().map_err(internal)
}

/// Streamed read with a hard byte ceiling; oversized or streaming-happy
/// responses fail closed instead of buffering unbounded bodies.
async fn read_bounded(mut response: reqwest::Response, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let chunk = match tokio::time::timeout(Duration::from_secs(5), response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => return Some(out),
            Ok(Err(_)) | Err(_) => return None,
        };
        if out.len() + chunk.len() > limit {
            return None;
        }
        out.extend_from_slice(&chunk);
    }
}

/// Probe bodies are tiny; anything larger means the URL is not serving
/// what we wrote, not that we should buffer it.
const PROBE_READ_LIMIT: usize = 1 << 16;

/// `scheme://authority/path` split at the first `/`.
fn authority_of(public_base_url: &str) -> Option<String> {
    let without_scheme = public_base_url.split_once("://")?.1;
    let authority = without_scheme.split('/').next()?;
    if authority.is_empty() {
        return None;
    }
    Some(authority.to_owned())
}

/// Bracketed IPv6 literal or bare host, without the port.
fn authority_host(authority: &str) -> String {
    if let Some(rest) = authority.strip_prefix('[') {
        if let Some((literal, _)) = rest.split_once(']') {
            return literal.to_owned();
        }
        return authority.to_owned();
    }
    match authority.split_once(':') {
        Some((host, _)) if !host.is_empty() => host.to_owned(),
        _ => authority.to_owned(),
    }
}

fn authority_port(authority: &str) -> Option<u16> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (_, after_bracket) = rest.split_once(']')?;
        return after_bracket.strip_prefix(':')?.parse().ok();
    }
    authority.split_once(':')?.1.parse().ok()
}

/// Anything loopback, private, link-local or otherwise unusable as a
/// genuine public media address refuses verification. IPv4-mapped
/// addresses are judged by their embedded IPv4 half.
fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(&v4.octets()),
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            let is_mapped =
                segments[0] == 0 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0;
            if is_mapped && segments[4] == 0 && segments[5] == 0xffff {
                let v4 = [
                    (segments[6] >> 8) as u8,
                    (segments[6] & 0xff) as u8,
                    (segments[7] >> 8) as u8,
                    (segments[7] & 0xff) as u8,
                ];
                return is_public_ipv4(&v4);
            }
            if is_mapped {
                // `::x` outside the mapped range never names a real host.
                return false;
            }
            let first = segments[0];
            !(first & 0xfe00 == 0xfc00       // unique local (fc00::/7)
                || first & 0xff00 == 0xfe00  // link local
                || first & 0xff00 == 0xff00) // multicast
        }
    }
}

fn is_public_ipv4(octets: &[u8]) -> bool {
    match octets {
        [0, ..] | [10, ..] | [127, ..] | [224..=255, ..] => false,
        [100, second, ..] => !(64..=127).contains(second), // shared NAT
        [169, 254, ..] => false,                           // link local
        [172, second, ..] => !(16..=31).contains(second),  // private
        [192, 168, ..] | [192, 0, 2, ..] | [198, 51, 100, ..] | [203, 0, 113, ..] => false,
        [198, second, ..] => !(18..=19).contains(second), // benchmarking
        _ => true,
    }
}

struct R2ObjectStore {
    base_url: String,
    bucket: String,
    access_key_id: String,
    secret_key: String,
    client: reqwest::Client,
    clock: Arc<dyn Clock>,
}

impl ObjectStore for R2ObjectStore {
    fn put<'a>(&'a self, key: &'a str, body: &'a [u8]) -> StoreFuture<'a> {
        Box::pin(async move {
            let request = self.signed_request("PUT", key, body).await?;
            let response = self.client.execute(request).await.map_err(internal)?;
            if !response.status().is_success() {
                return Err(internal(format!(
                    "object put failed with status {}",
                    response.status().as_str()
                )));
            }
            Ok(())
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a> {
        Box::pin(async move {
            let request = self.signed_request("DELETE", key, b"").await?;
            let response = self.client.execute(request).await.map_err(internal)?;
            if response.status() == 404 {
                // The desired end state is already reached.
                return Ok(());
            }
            if !response.status().is_success() {
                return Err(internal(format!(
                    "object delete failed with status {}",
                    response.status().as_str()
                )));
            }
            Ok(())
        })
    }
}

impl R2ObjectStore {
    /// Builds one signed request against the single canonical set of
    /// headers. The payload hash is the real SHA-256 of the body, so
    /// integrity coverage is exact and no unsigned-payload edge cases
    /// exist.
    async fn signed_request(
        &self,
        method: &str,
        key: &str,
        body: &[u8],
    ) -> Result<reqwest::Request, Error> {
        let canonical_uri = object_uri(&self.bucket, key);
        let zulu = iso_8601_zulu(self.clock.now());
        let amz_date = zulu.format_amz_datetime();
        let credential_date = zulu.format_credential_date();
        let authority = self
            .base_url
            .split_once("://")
            .map(|(_, rest)| rest.to_owned())
            .ok_or_else(|| internal("storage endpoint has no scheme"))?;

        let payload_hash = hex(&sha2::Sha256::digest(body));
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n\nhost:{authority}\n\
             x-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n\n\
             {SIGNED_HEADERS}\n{payload_hash}"
        );
        let scope = format!("{credential_date}/{SIGNING_REGION}/{SIGNING_SERVICE}/aws4_request");
        let string_to_sign = format!(
            "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
            hex(&sha2::Sha256::digest(canonical_request.as_bytes()))
        );
        let signing_key = signing_key(&self.secret_key, &credential_date);
        let signature = hex(&hmac_sha256(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "{ALGORITHM} Credential={}/{scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}",
            self.access_key_id
        );

        // The signed payload hash only counts if the request actually
        // carries the bytes: PUT bodies ride on the request itself.
        let mut request = self
            .client
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).map_err(internal)?,
                format!("{}{canonical_uri}", self.base_url),
            )
            .body(body.to_vec())
            .build()
            .map_err(internal)?;
        let headers = request.headers_mut();
        let names = [
            (reqwest::header::AUTHORIZATION, authorization),
            (
                reqwest::header::HeaderName::from_static("x-amz-content-sha256"),
                payload_hash,
            ),
            (
                reqwest::header::HeaderName::from_static("x-amz-date"),
                amz_date,
            ),
        ];
        for (name, value) in names {
            let parsed = reqwest::header::HeaderValue::try_from(value)
                .map_err(|e| internal(e.to_string()))?;
            headers.insert(name, parsed);
        }
        Ok(request)
    }
}

/// `/bucket/encoded/key...`: the S3 path-style object address, bucket
/// included, so the signed and sent requests reference the same resource.
fn object_uri(bucket: &str, key: &str) -> String {
    let mut uri = String::with_capacity(bucket.len() + key.len() + 2);
    uri.push('/');
    uri.push_str(bucket);
    for segment in key.split('/') {
        uri.push('/');
        uri.push_str(&encoded_segment(segment));
    }
    uri
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac =
        <Hmac<sha2::Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    let out = mac.finalize().into_bytes();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&out);
    bytes
}

/// AWS SigV4 sub-key derivation for R2's fixed region/service.
fn signing_key(secret: &str, date: &str) -> [u8; 32] {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, SIGNING_REGION.as_bytes());
    let k_service = hmac_sha256(&k_region, SIGNING_SERVICE.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

/// Percent-encodes one key segment, keeping unreserved bytes intact.
fn encoded_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Public URL for one object path: percent-encoded segments appended to the
/// canonical base, which never carries a query or trailing slash.
pub fn format_public_url(public_base_url: &str, path: &str) -> Option<String> {
    if public_base_url.ends_with('/') {
        return None;
    }
    let encoded = path
        .split('/')
        .map(encoded_segment)
        .collect::<Vec<_>>()
        .join("/");
    Some(format!("{public_base_url}/{encoded}"))
}

/// `YYYYMMDDThhmmssZ` plus `YYYYMMDD` from epoch seconds, without a date
/// library.
pub struct Iso8601Zulu {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

impl Iso8601Zulu {
    /// The request-signing timestamp format.
    pub fn format_amz_datetime(&self) -> String {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// The credential scope date format.
    pub fn format_credential_date(&self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

/// Civil-from-days conversion (Howard Hinnant): pure integer arithmetic,
/// valid across realistic signing timestamps.
pub fn iso_8601_zulu(epoch_seconds: i64) -> Iso8601Zulu {
    let days = epoch_seconds.div_euclid(86_400);
    let secs_of_day = epoch_seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    Iso8601Zulu {
        year,
        month: month.try_into().unwrap_or(1),
        day: day.try_into().unwrap_or(1),
        hour: (secs_of_day / 3_600).try_into().unwrap_or(0),
        minute: (secs_of_day % 3_600 / 60).try_into().unwrap_or(0),
        second: (secs_of_day % 60).try_into().unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn zulu_conversion_matches_known_timestamps() {
        let t = iso_8601_zulu(1_700_000_000);
        assert_eq!(t.format_amz_datetime(), "20231114T221320Z");
        assert_eq!(t.format_credential_date(), "20231114");
        let leap_day = iso_8601_zulu(1_709_164_800);
        assert_eq!(leap_day.format_credential_date(), "20240229");
    }

    #[test]
    fn public_ip_decisions() {
        let public = |text: &str| is_public_ip(IpAddr::from_str(text).expect("ip"));
        for refused in [
            "127.0.0.1",
            "10.0.0.9",
            "172.16.5.5",
            "172.31.255.255",
            "192.168.1.1",
            "192.0.2.7",
            "198.51.100.7",
            "203.0.113.7",
            "198.18.0.9",
            "169.254.1.1",
            "0.0.0.0",
            "100.64.0.1",
            "100.127.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "ff02::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public(refused), "{refused} must not count as public");
        }
        for allowed in [
            "8.8.8.8",
            "1.1.1.1",
            "100.128.0.1",
            "2600::1",
            "2a01::1",
            "2606:4700::6810:85e5",
        ] {
            assert!(public(allowed), "{allowed} should count as public");
        }
    }

    #[test]
    fn hex_encoding_is_lowercase_even_width() {
        assert_eq!(hex(&[0x0f, 0xff, 0x00]), "0fff00");
    }

    #[test]
    fn segments_percent_encode_reserved_bytes() {
        assert_eq!(encoded_segment("statesu/blogs a"), "statesu%2Fblogs%20a");
        assert_eq!(encoded_segment("safe-key.1_2~3"), "safe-key.1_2~3");
    }

    #[test]
    fn public_urls_are_literal_and_quiet() {
        let url = format_public_url("https://media.example.com", "blogs/a b/x.png");
        assert_eq!(
            url.as_deref(),
            Some("https://media.example.com/blogs/a%20b/x.png")
        );
        assert!(format_public_url("https://host/", "k").is_none());
    }

    #[test]
    fn authority_parts_parse() {
        assert_eq!(
            authority_host("media.example.com:8443"),
            "media.example.com"
        );
        assert_eq!(authority_port("media.example.com:8443"), Some(8443));
        assert_eq!(authority_port("media.example.com"), None);
        assert_eq!(authority_host("[2001:db8::1]:443"), "2001:db8::1");
        assert_eq!(authority_port("[2001:db8::1]:443"), Some(443));
    }
}
