use std::sync::Arc;

use uuid::Uuid;

use crate::blog::BlogRow;
use crate::blog::repo::BlogRepository;
use crate::clock::Clock;
use crate::error::Error;
use crate::ids::{parse_uuid_v7, valid_slug};

/// Upper byte length for a blog name after trimming.
pub const MAX_NAME_BYTES: usize = 128;
/// Upper limit on blogs per user and origins per blog, so no user can
/// exhaust storage through many small entities.
pub const MAX_BLOGS_PER_USER: usize = 20;
pub const MAX_ORIGINS_PER_BLOG: usize = 32;

/// Wire shape for create/list/update and origins responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlogResponse {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub slug: String,
    pub domain: Option<String>,
    pub origins: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Wire shape for the origins replacement response.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OriginsResponse {
    pub origins: Vec<String>,
}

/// Wire shape for the public delivery API: no owner IDs, no management data.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PublicBlogResponse {
    pub name: String,
    pub slug: String,
    pub domain: Option<String>,
}

/// Strict JSON request body for creating a blog.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateBlogRequest {
    pub name: String,
    pub slug: String,
    pub domain: Option<String>,
}

/// Strict JSON request body for patching a blog; absent fields stay
/// unchanged, an explicit `domain: null` clears the domain.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateBlogRequest {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_clearable_domain")]
    pub domain: Option<Option<String>>,
}

/// Serde collapses `Option<Option<T>>`: missing and explicit null both
/// deserialize to `None`, which would make "leave unchanged" and "clear"
/// indistinguishable. This visitor keeps the two apart.
fn deserialize_clearable_domain<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Visitor;

    struct DomainVisitor;

    impl<'de> Visitor<'de> for DomainVisitor {
        type Value = Option<Option<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a domain string, or null to clear")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(Some(None))
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let inner = <Option<String> as serde::Deserialize>::deserialize(deserializer)?;
            Ok(Some(inner))
        }
    }

    deserializer.deserialize_option(DomainVisitor)
}

/// Strict JSON request body for replacing the origin list.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceOriginsRequest {
    pub origins: Vec<String>,
}

/// Blog rules: field validation, domain/origin normalization, ownership and
/// limit enforcement. Repos are injected; no globals.
#[derive(Clone)]
pub struct BlogService {
    blogs: BlogRepository,
    clock: Arc<dyn Clock>,
}

impl BlogService {
    pub fn new(blogs: BlogRepository, clock: Arc<dyn Clock>) -> BlogService {
        BlogService { blogs, clock }
    }

    pub async fn create(
        &self,
        owner_id: Uuid,
        request: &CreateBlogRequest,
    ) -> Result<BlogResponse, Error> {
        let name = valid_name(&request.name)?;
        if !valid_slug(&request.slug) {
            return Err(Error::InvalidInput);
        }
        let domain = normalize_domain(request.domain.as_deref())?;
        let count = self.blogs.count_for_owner(&owner_id).await?;
        if count >= i64::try_from(MAX_BLOGS_PER_USER).unwrap_or(i64::MAX) {
            return Err(Error::Forbidden);
        }
        let now = self.clock.now();
        let id = Uuid::now_v7();
        self.blogs
            .insert(&id, &owner_id, &name, &request.slug, domain.as_deref(), now)
            .await?;
        Ok(BlogResponse {
            id,
            owner_id,
            name,
            slug: request.slug.clone(),
            domain,
            origins: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn list_for_owner(&self, owner_id: Uuid) -> Result<Vec<BlogResponse>, Error> {
        let rows = self.blogs.list_for_owner(&owner_id).await?;
        let mut responses = Vec::with_capacity(rows.len());
        for row in rows {
            responses.push(self.blog_response(row).await?);
        }
        Ok(responses)
    }

    /// Ownership is verified before any mutation; a foreign or unknown blog
    /// is always `BlogNotFound`.
    pub async fn update(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
        request: &UpdateBlogRequest,
    ) -> Result<BlogResponse, Error> {
        let blog_id = parse_uuid_v7(blog_id_param)?;
        let mut blog = self
            .blogs
            .find_for_owner(&owner_id, &blog_id)
            .await?
            .ok_or(Error::BlogNotFound)?;
        let name = match &request.name {
            Some(name) => valid_name(name)?,
            None => blog.name.clone(),
        };
        let domain = match &request.domain {
            Some(Some(domain)) => normalize_domain(Some(domain))?,
            Some(None) => None,
            None => blog.domain.clone(),
        };
        let now = self.clock.now();
        self.blogs
            .update(&blog_id, &owner_id, &name, domain.as_deref(), now)
            .await?;
        blog.name = name;
        blog.domain = domain;
        blog.updated_at = now;
        self.blog_response(blog).await
    }

    /// Replaces the whole origin list with normalized values; the blog must
    /// belong to the caller.
    pub async fn set_origins(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
        request: &ReplaceOriginsRequest,
    ) -> Result<Vec<String>, Error> {
        if request.origins.len() > MAX_ORIGINS_PER_BLOG {
            return Err(Error::Forbidden);
        }
        let blog_id = parse_uuid_v7(blog_id_param)?;
        let blog = self
            .blogs
            .find_for_owner(&owner_id, &blog_id)
            .await?
            .ok_or(Error::BlogNotFound)?;
        let origins = &request.origins;
        let mut normalized: Vec<String> = origins
            .iter()
            .map(|origin| normalize_origin(origin))
            .collect::<Result<_, _>>()?;
        normalized.sort();
        normalized.dedup();
        self.blogs
            .replace_origins(&blog.id, normalized.clone())
            .await?;
        Ok(normalized)
    }

    /// Exact origins allowed to call the delivery API for this slug; unknown
    /// blogs read as empty.
    pub async fn origins_for_slug(&self, slug: &str) -> Result<Vec<String>, Error> {
        self.blogs.origins_for_slug(slug).await
    }

    pub async fn public_blog(&self, slug: &str) -> Result<PublicBlogResponse, Error> {
        self.blogs
            .find_by_slug(slug)
            .await?
            .map(|row| PublicBlogResponse {
                name: row.name,
                slug: row.slug,
                domain: row.domain,
            })
            .ok_or(Error::BlogNotFound)
    }

    async fn blog_response(&self, row: BlogRow) -> Result<BlogResponse, Error> {
        Ok(BlogResponse {
            origins: self.blogs.origins_for_slug(&row.slug).await?,
            id: row.id,
            owner_id: row.owner_id,
            name: row.name,
            slug: row.slug,
            domain: row.domain,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Names are trimmed, single-line, printable and bounded so no user can
/// exhaust storage or corrupt listings with control characters.
fn valid_name(name: &str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name
            .chars()
            .any(|ch| !(0x20..=0x7E).contains(&(ch as u32)) && !(ch as u32 >= 0xA0))
    {
        return Err(Error::InvalidInput);
    }
    Ok(name.to_owned())
}

/// Domains are stored as the hostname (optionally `host:port`): lowercase,
/// without scheme, path, query or credentials. This is metadata for the blog
/// profile; browser access control happens on normalized origins.
fn normalize_domain(input: Option<&str>) -> Result<Option<String>, Error> {
    let Some(input) = input.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    // Only web schemes are meaningful for a blog domain; anything else is a
    // mistake worth rejecting.
    let without_scheme = if let Some((scheme, rest)) = input.split_once("://") {
        if !matches!(scheme, "http" | "https" | "HTTP" | "HTTPS") {
            return Err(Error::InvalidInput);
        }
        rest
    } else {
        input
    };
    let authority = authority_of(without_scheme)?;
    Ok(Some(authority.to_ascii_lowercase()))
}

/// A browser origin is `scheme://host[:port]` with no path. The normalized
/// form lowercases scheme and host so origin comparisons stay exact but
/// case-insensitive.
pub fn normalize_origin(input: &str) -> Result<String, Error> {
    let input = input.trim();
    let (scheme, rest) = input.split_once("://").ok_or(Error::InvalidInput)?;
    if scheme != "http" && scheme != "https" && scheme != "HTTP" && scheme != "HTTPS" {
        return Err(Error::InvalidInput);
    }
    // An origin never carries a path, query or fragment.
    let authority = authority_of(rest)?;
    if rest != authority {
        return Err(Error::InvalidInput);
    }
    Ok(format!("{scheme}://{authority}").to_ascii_lowercase())
}

/// `host[:port]` with no path, query or credentials. The host is a strict
/// RFC 1123 hostname (ASCII labels, optional punycode `xn--` for IDN), an
/// IPv4-like all-numeric name, or a bracketed IPv6 literal. Trailing whitespace
/// must have been trimmed by the caller. The value is returned verbatim and
/// lowercase is applied by the caller where scheme matters.
fn authority_of(input: &str) -> Result<String, Error> {
    let authority = input
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim_end_matches('.');
    if authority.is_empty() || authority.contains('@') || authority.contains(' ') {
        return Err(Error::InvalidInput);
    }
    let (host, port) = split_host_port(authority)?;
    if !valid_host(&host) {
        return Err(Error::InvalidInput);
    }
    if let Some(port) = port {
        validate_port(&port)?;
    }
    Ok(authority.to_owned())
}

/// Splits `host[:port]`, keeping bracketed IPv6 literals intact.
fn split_host_port(authority: &str) -> Result<(String, Option<String>), Error> {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((literal, remainder)) = rest.split_once(']') else {
            return Err(Error::InvalidInput);
        };
        let port = remainder.strip_prefix(':').map(str::to_owned);
        return Ok((format!("[{literal}]"), port));
    }
    match authority.split_once(':') {
        Some((host, port)) => Ok((host.to_owned(), Some(port.to_owned()))),
        None => Ok((authority.to_owned(), None)),
    }
}

/// Ports must be decimal digits in 1..=65535.
fn validate_port(port: &str) -> Result<(), Error> {
    let value: u16 = port.parse().map_err(|_| Error::InvalidInput)?;
    if value == 0 {
        return Err(Error::InvalidInput);
    }
    Ok(())
}

/// Either a hostname ([RFC 1123](https://datatracker.ietf.org/doc/html/rfc1123))
/// or an IPv6 literal in brackets.
fn valid_host(host: &str) -> bool {
    let inner = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'));
    match inner {
        Some(literal) => literal.parse::<std::net::Ipv6Addr>().is_ok(),
        None => valid_hostname(host),
    }
}

/// RFC 1123 hostname: 253 bytes max, dot-separated ASCII labels of 1..=63
/// bytes, letters/digits/hyphens, hyphens only between characters. Punycode
/// (`xn--…`) labels and all-numeric (IPv4-like) names are accepted.
fn valid_hostname(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.split('.').all(valid_hostname_label)
}

fn valid_hostname_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

#[cfg(test)]
mod host_tests {
    use super::*;

    #[test]
    fn origins_accept_hostnames_idn_and_ipv6_literals() {
        for origin in [
            "https://example.com",
            "http://EXAMPLE.com:8080",
            "https://localhost",
            "http://localhost:5173",
            "https://xn--bcher-kva.example.com",
            "http://127.0.0.1:3000",
            "http://[::1]:5173",
            "https://[2001:db8::1]",
        ] {
            assert!(normalize_origin(origin).is_ok(), "should accept {origin}");
        }
        assert_eq!(
            normalize_origin("HTTP://EXAMPLE.com:8080").unwrap(),
            "http://example.com:8080"
        );
    }

    #[test]
    fn origins_reject_malformed_input() {
        for origin in [
            "",
            "  ",
            "example.com",               // missing scheme
            "ftp://example.com",         // wrong scheme
            "https://",                  // no host
            "https://example.com/",      // trailing slash
            "https://example.com/path",  // path
            "https://example.com:0",     // port 0
            "https://example.com:65536", // out of range
            "https://example.com:",      // empty port
            "https://exa mple.com",      // space
            "https://user@example.com",  // credentials
            "https://a-.example",        // label ends with hyphen
            "https://-a.example",        // label starts with hyphen
            "https://a_b.example",       // underscore
            "https://[::1",              // unbalanced bracket
            "https://[not-ip6]",         // not an IPv6 literal
            "https://a..b",              // empty label
        ] {
            assert!(
                normalize_origin(origin).is_err(),
                "should reject {origin:?}"
            );
        }
    }

    #[test]
    fn domains_strip_web_schemes_only_and_validate_hosts() {
        assert_eq!(
            normalize_domain(Some("HTTPS://Example.com/posts")).unwrap(),
            Some("example.com".to_owned())
        );
        assert!(normalize_domain(Some("my_site.example")).is_err());
        assert!(normalize_domain(Some("ftp://example.com")).is_err());
        assert_eq!(normalize_domain(Some("")).unwrap(), None);
        assert_eq!(normalize_domain(Some("   ")).unwrap(), None);
    }
}
