//! Media domain: user-connectable R2 storage ("storage connections"), the
//! per-blog storage preference, and uploaded media rows. Field rules live
//! here so the server's own bucket configuration and user-created
//! connections share one validation contract.

pub mod media_repo;
pub mod process;
pub mod repo;
pub mod service;

pub use service::{MediaPublicMeta, MediaResponse, MediaVariantResponse};

use uuid::Uuid;

use crate::error::Error;
use crate::ids::parse_uuid_v7;
use crate::storage::BucketConnection;

/// Upper limits for one storage connection; connection counts are per blog.
pub const MAX_CONNECTIONS_PER_BLOG: usize = 8;
pub const MAX_NAME_BYTES: usize = 128;
const MAX_CREDENTIAL_BYTES: usize = 256;
const MAX_PREFIX_BYTES: usize = 256;
const MAX_PUBLIC_BASE_URL_BYTES: usize = 512;
/// Cloudflare account IDs are always 32 hex characters.
const ACCOUNT_ID_LEN: usize = 32;

/// Media object keys are absolute (`blogs/{blog_id}/{media_id}/{label}.{ext}`);
/// public Markdown embeds exactly `{base}/{key}`. The scan looks for the
/// shape `/blogs/{uuid}/{uuid}`, keeps only UUIDv7-shaped media ids and
/// thereby never mistakes delivery URLs (`/public/blogs/{slug}`) for
/// object paths.
pub fn referenced_media_ids(contents: &[&str]) -> Vec<Uuid> {
    const MAX_REFERENCED_MEDIA: usize = 64;
    let mut ids: Vec<Uuid> = Vec::new();
    for content in contents {
        let mut rest: &str = content;
        while let Some(position) = rest.find("/blogs/") {
            let after: &str = &rest[position + "/blogs/".len()..];
            // First segment inside an object path is the blog id.
            let blog_end = first_token_end(after);
            if Uuid::parse_str(&after[..blog_end]).is_ok() {
                let media_part = after.get(blog_end + 1..).unwrap_or("");
                let media_end = first_token_end(media_part);
                if let Ok(id) = Uuid::parse_str(&media_part[..media_end])
                    && id.get_version_num() == 7
                    && !ids.contains(&id)
                    && ids.len() < MAX_REFERENCED_MEDIA
                {
                    ids.push(id);
                }
            }
            rest = after;
        }
    }
    ids
}

/// The first token of a URL path segment: until a slash, quote, closing
/// parenthesis, query or fragment marker, or end of input.
fn first_token_end(text: &str) -> usize {
    text.find(['/', '"', ')', '?', '#']).unwrap_or(text.len())
}

/// Storage shape for the `storage_connections` table.
#[derive(Debug, Clone)]
pub struct StorageConnectionRow {
    pub id: Uuid,
    pub blog_id: Uuid,
    pub owner_id: Uuid,
    pub name: Option<String>,
    pub account_id: String,
    pub bucket: String,
    pub prefix: Option<String>,
    pub public_base_url: String,
    pub access_key_id: String,
    /// AES-GCM sealed secret, AAD-bound to `id`.
    pub secret_key: Vec<u8>,
    pub verified_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `sqlx::FromRow` projection before ID validation.
#[derive(Debug, sqlx::FromRow)]
pub struct RawConnectionRow {
    id: String,
    blog_id: String,
    owner_id: String,
    name: Option<String>,
    account_id: String,
    bucket: String,
    prefix: Option<String>,
    public_base_url: String,
    access_key_id: String,
    secret_key: Vec<u8>,
    verified_at: i64,
    created_at: i64,
    updated_at: i64,
}

impl RawConnectionRow {
    pub fn into_row(self) -> Result<StorageConnectionRow, Error> {
        Ok(StorageConnectionRow {
            id: parse_uuid_v7(&self.id)?,
            blog_id: parse_uuid_v7(&self.blog_id)?,
            owner_id: parse_uuid_v7(&self.owner_id)?,
            name: self.name,
            account_id: self.account_id,
            bucket: self.bucket,
            prefix: self.prefix,
            public_base_url: self.public_base_url,
            access_key_id: self.access_key_id,
            secret_key: self.secret_key,
            verified_at: self.verified_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

impl StorageConnectionRow {
    /// Wire shape without any credential material.
    pub fn to_response(&self) -> StorageConnectionResponse {
        StorageConnectionResponse {
            id: self.id,
            blog_id: self.blog_id,
            name: self.name.clone(),
            account_id: self.account_id.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
            public_base_url: self.public_base_url.clone(),
            verified_at: self.verified_at,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Wire shape for storage-connection responses. No credential material
/// ships under any circumstance.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StorageConnectionResponse {
    pub id: Uuid,
    pub blog_id: Uuid,
    pub name: Option<String>,
    pub account_id: String,
    pub bucket: String,
    pub prefix: Option<String>,
    pub public_base_url: String,
    pub verified_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Strict JSON body for creating a connection. Secrets travel only in this
/// request and never back out in any response.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateConnectionRequest {
    pub name: Option<String>,
    pub account_id: String,
    pub bucket: String,
    pub prefix: Option<String>,
    pub public_base_url: String,
    pub access_key_id: String,
    pub secret_key: String,
}

/// Strict JSON body for patching a connection. Credential rotation happens
/// as a pair: providing only one side keeps the stored pair unusable, so it
/// is rejected.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateConnectionRequest {
    pub name: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_key: Option<String>,
}

/// Strict JSON body for replacing the per-blog storage preference; a
/// missing or null `connection_id` selects the server bucket.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceStorageRequest {
    pub connection_id: Option<Uuid>,
}

/// Wire shape for the per-blog storage status.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlogStorageResponse {
    /// `statesu` (server bucket) or `custom` (user connection).
    pub storage: String,
    /// Which custom connection applies; null for the server bucket.
    pub connection_id: Option<Uuid>,
}

/// Cloudflare account IDs: exactly 32 hex characters, case normalized.
pub fn validate_account_id(input: &str) -> Result<String, Error> {
    let normalized = input.trim().to_ascii_lowercase();
    if normalized.len() != ACCOUNT_ID_LEN
        || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::InvalidInput);
    }
    Ok(normalized)
}

/// R2 bucket names: 3..=63 lowercase letters, digits, hyphens and
/// underscores, separators inside only.
pub fn validate_bucket_name(input: &str) -> Result<String, Error> {
    let input = input.trim();
    let edge_separator = |input: &str| {
        input.starts_with(['-', '_']) || input.ends_with(['-', '_']) || input.contains("__")
    };
    if !(3..=63).contains(&input.len())
        || !input.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
        || edge_separator(input)
    {
        return Err(Error::InvalidInput);
    }
    Ok(input.to_owned())
}

/// The Statesu-dedicated key prefix inside a bucket: relative path segments
/// without leading or trailing slashes and without `..` anywhere.
pub fn normalize_prefix(input: Option<&str>) -> Result<Option<String>, Error> {
    let Some(input) = input.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let input = input.trim_matches('/');
    if input.len() > MAX_PREFIX_BYTES
        || input.split('/').any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return Err(Error::InvalidInput);
    }
    Ok(Some(input.to_owned()))
}

/// Public media base: `https://authority[/path]`, no query, no credentials,
/// no trailing slash. The path keeps its original case.
pub fn validate_public_base_url(input: &str) -> Result<String, Error> {
    let input = input.trim();
    if input.is_empty() || input.len() > MAX_PUBLIC_BASE_URL_BYTES {
        return Err(Error::InvalidInput);
    }
    let uri: http::Uri = input.parse().map_err(|_| Error::InvalidInput)?;
    if uri.scheme_str() != Some("https") {
        return Err(Error::InvalidInput);
    }
    let authority = uri.authority().ok_or(Error::InvalidInput)?;
    if authority.as_str().contains('@') {
        return Err(Error::InvalidInput);
    }
    if uri.query().is_some() {
        return Err(Error::InvalidInput);
    }
    let path = uri.path().trim_end_matches('/');
    for (index, segment) in path.split('/').enumerate() {
        if index == 0 {
            // The leading empty segment is the path root.
            continue;
        }
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(Error::InvalidInput);
        }
    }
    Ok(format!("https://{authority}{path}"))
}

/// Runtime reach of one connection row: the bucket plus the decrypted
/// credential pair. Secrets are opened right before use, never stored.
pub(crate) fn connection_material(
    row: &StorageConnectionRow,
    secrets: &crate::crypto::secret::SecretCipher,
) -> Result<BucketConnection, Error> {
    let secret = secrets.open(&row.secret_key, &row.id)?;
    let secret = String::from_utf8(secret).map_err(|e| {
        Error::Internal(crate::error::InternalError::new(std::io::Error::other(
            e.to_string(),
        )))
    })?;
    Ok(BucketConnection {
        account_id: row.account_id.clone(),
        bucket: row.bucket.clone(),
        prefix: row.prefix.clone().unwrap_or_default(),
        access_key_id: row.access_key_id.clone(),
        secret_key: secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_id_scan_pulls_media_paths_only() {
        let blog = Uuid::now_v7();
        let media = Uuid::now_v7();
        let object_url = format!("https://media.example/statesu/blogs/{blog}/{media}/source.png");
        let content =
            format!("![image]({object_url}){{width=640}} and /public/blogs/my-slug links");
        assert_eq!(referenced_media_ids(&[&content]), vec![media]);

        // A flat /blogs/{uuid} path without a media segment is not an
        // object path; nil (non-v7) ids are not media ids either.
        let flat = format!("https://x.example/blogs/{media}/source.png");
        assert!(referenced_media_ids(&[&flat]).is_empty());
        let flat_nil = format!("https://x.example/blogs/{blog}/{}/source.png", Uuid::nil());
        assert!(referenced_media_ids(&[&flat_nil]).is_empty());

        // Deduplication across posts stays in scan order.
        let second = Uuid::now_v7();
        let second_url = format!("![b](https://m.example/blogs/{blog}/{second}/w640.png)");
        let results = referenced_media_ids(&[&object_url, &second_url, &object_url]);
        assert_eq!(results, vec![media, second]);
        // Unrelated text never starts a scan.
        assert!(referenced_media_ids(&["no mentions at all"]).is_empty());
    }

    #[test]
    fn account_ids_are_32_hex_lowercased() {
        assert_eq!(
            validate_account_id(" ABCDEF0123456789ABCDEF0123456789").unwrap(),
            "abcdef0123456789abcdef0123456789"
        );
        assert!(validate_account_id("short").is_err());
        assert!(validate_account_id(&"g".repeat(32)).is_err());
        assert!(validate_account_id("").is_err());
    }

    #[test]
    fn bucket_names_are_r2_shaped() {
        assert_eq!(validate_bucket_name("my-bucket_1").unwrap(), "my-bucket_1");
        for bad in [
            "ab",            // too short
            "-lead",         // leading separator
            "trail-",        // trailing separator
            "a__b",          // doubles
            "UPPER",         // case
            "with space",    // space
            &"a".repeat(64), // too long
        ] {
            assert!(validate_bucket_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn prefixes_are_relative_key_paths() {
        assert_eq!(
            normalize_prefix(Some("statesu/media")).unwrap(),
            Some("statesu/media".to_owned())
        );
        assert_eq!(
            normalize_prefix(Some("/leading/")).unwrap(),
            Some("leading".to_owned())
        );
        assert_eq!(normalize_prefix(Some("  ")).unwrap(), None);
        assert_eq!(normalize_prefix(None).unwrap(), None);
        assert!(normalize_prefix(Some("a//b")).is_err());
        assert!(normalize_prefix(Some("a/./b")).is_err());
        assert!(normalize_prefix(Some("a/../b")).is_err());
        assert!(normalize_prefix(Some("a b")).is_err());
    }

    #[test]
    fn public_base_urls_are_https_origins() {
        assert_eq!(
            validate_public_base_url("HTTPS://Media.Example.com/").unwrap(),
            "https://Media.Example.com"
        );
        assert_eq!(
            validate_public_base_url("https://media.example.com/cdn/media").unwrap(),
            "https://media.example.com/cdn/media"
        );
        assert_eq!(
            validate_public_base_url("https://host/media/").unwrap(),
            "https://host/media"
        );
        for bad in [
            "http://insecure.example.com",
            "https://",
            "https://user@host",
            "https://host?query",
            "https://host/media//double",
            "https://host/media/../media",
            "ftp://host",
            "",
        ] {
            assert!(
                validate_public_base_url(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
