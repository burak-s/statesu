use uuid::Uuid;

use crate::error::Error;

/// Path and body IDs are validated as UUIDv7 before they reach SQL.
pub fn parse_uuid_v7(text: &str) -> Result<Uuid, Error> {
    let parsed = text.parse::<Uuid>().map_err(|_| Error::InvalidInput)?;
    if parsed.get_version_num() == 7 {
        Ok(parsed)
    } else {
        Err(Error::InvalidInput)
    }
}

/// Slugs are lowercase ASCII, digits and hyphens, with hyphens only between
/// characters — simple to type, URL-safe and deterministic.
pub fn valid_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}
