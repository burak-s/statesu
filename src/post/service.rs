use std::sync::Arc;

use uuid::Uuid;

use crate::blog::repo::BlogRepository;
use crate::clock::Clock;
use crate::error::Error;
use crate::ids::parse_uuid_v7;
use crate::pagination::{parse_page_number, parse_page_size};
use crate::post::repo::{NewPost, PostRepository};
use crate::post::{PostRow, STATUS_DRAFT, STATUS_PUBLISHED};

/// Upper byte lengths after trimming. Content bounds keep single imports and
/// updates from exhausting storage.
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_CONTENT_BYTES: usize = 256 * 1024;

/// Strict JSON request body for creating a post.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePostRequest {
    pub slug: String,
    pub title: String,
    pub content_markdown: String,
    pub status: Option<String>,
}

/// Strict JSON request body for patching a post; absent fields stay
/// unchanged.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePostRequest {
    pub slug: Option<String>,
    pub title: Option<String>,
    pub content_markdown: Option<String>,
    pub status: Option<String>,
}

/// Wire shape for management responses.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PostResponse {
    pub id: Uuid,
    pub blog_id: Uuid,
    pub author_id: Uuid,
    pub slug: String,
    pub title: String,
    pub content_markdown: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub published_at: Option<i64>,
}

/// Wire shape for public delivery responses: no management fields.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PublicPostResponse {
    pub slug: String,
    pub title: String,
    pub content_markdown: String,
    pub published_at: i64,
}

/// Shared list envelope for management and public listings.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub size: u32,
    pub total: i64,
}

/// Post rules: field validation, publication transitions, ownership and
/// pagination. Repos are injected; no globals.
#[derive(Clone)]
pub struct PostService {
    posts: PostRepository,
    blogs: BlogRepository,
    clock: Arc<dyn Clock>,
}

impl PostService {
    pub fn new(posts: PostRepository, blogs: BlogRepository, clock: Arc<dyn Clock>) -> PostService {
        PostService {
            posts,
            blogs,
            clock,
        }
    }

    /// Creates a draft by default; `status: "published"` publishes now.
    pub async fn create(
        &self,
        author_id: Uuid,
        blog_id_param: &str,
        request: &CreatePostRequest,
    ) -> Result<PostResponse, Error> {
        let blog = self.owned_blog(author_id, blog_id_param).await?;
        let slug = valid_slug_field(&request.slug)?;
        let title = valid_title(&request.title)?;
        let content = valid_content(&request.content_markdown)?;
        let status = valid_status(request.status.as_deref())?;
        let now = self.clock.now();
        let id = Uuid::now_v7();
        self.posts
            .insert(&NewPost {
                id: &id,
                blog_id: &blog.id,
                author_id: &author_id,
                slug: &slug,
                title: &title,
                content_markdown: &content,
                status,
                now,
            })
            .await?;
        Ok(PostResponse {
            id,
            blog_id: blog.id,
            author_id,
            slug,
            title,
            content_markdown: content,
            status: status.to_owned(),
            created_at: now,
            updated_at: now,
            published_at: (status == STATUS_PUBLISHED).then_some(now),
        })
    }

    pub async fn page_for_blog(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
        page_param: Option<&str>,
        size_param: Option<&str>,
    ) -> Result<ListResponse<PostResponse>, Error> {
        let blog = self.owned_blog(owner_id, blog_id_param).await?;
        let page = parse_page_number(page_param)?;
        let size = parse_page_size(size_param)?;
        // Page and size are u32 with size <= 100, so the offset stays far
        // inside the i64 range SQLite expects.
        let offset = u64::from(page.saturating_sub(1)) * u64::from(size);
        let rows = self
            .posts
            .page_for_blog(
                &blog.id,
                i64::from(size),
                i64::try_from(offset).map_err(|_| Error::InvalidInput)?,
            )
            .await?;
        let total = self.posts.count_for_blog(&blog.id).await?;
        Ok(ListResponse {
            items: rows.into_iter().map(post_response).collect(),
            page,
            size,
            total,
        })
    }

    pub async fn get(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
        post_id_param: &str,
    ) -> Result<PostResponse, Error> {
        let blog = self.owned_blog(owner_id, blog_id_param).await?;
        let post_id = parse_uuid_v7(post_id_param)?;
        let row = self
            .posts
            .find(&blog.id, &post_id)
            .await?
            .ok_or(Error::PostNotFound)?;
        Ok(post_response(row))
    }

    /// Patches fields and applies the publication transition on the loaded
    /// row, then writes the whole row back.
    pub async fn update(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
        post_id_param: &str,
        request: &UpdatePostRequest,
    ) -> Result<PostResponse, Error> {
        let blog = self.owned_blog(owner_id, blog_id_param).await?;
        let post_id = parse_uuid_v7(post_id_param)?;
        let mut row = self
            .posts
            .find(&blog.id, &post_id)
            .await?
            .ok_or(Error::PostNotFound)?;
        if let Some(slug) = &request.slug {
            row.slug = valid_slug_field(slug)?;
        }
        if let Some(title) = &request.title {
            row.title = valid_title(title)?;
        }
        if let Some(content) = &request.content_markdown {
            row.content_markdown = valid_content(content)?;
        }
        let next_status = valid_status(Some(request.status.as_deref().unwrap_or(&row.status)))?;
        let now = self.clock.now();
        match (row.status.as_str(), next_status) {
            (STATUS_DRAFT, STATUS_PUBLISHED) => row.published_at = Some(now),
            (STATUS_PUBLISHED, STATUS_DRAFT) => row.published_at = None,
            _ => {}
        }
        row.status = next_status.to_owned();
        row.updated_at = now;
        self.posts.update(&row).await?;
        Ok(post_response(row))
    }

    /// Soft delete: the row stays for stable slugs and disappears from every
    /// listing and read.
    pub async fn delete(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
        post_id_param: &str,
    ) -> Result<(), Error> {
        let blog = self.owned_blog(owner_id, blog_id_param).await?;
        let post_id = parse_uuid_v7(post_id_param)?;
        let deleted = self
            .posts
            .soft_delete(&blog.id, &post_id, self.clock.now())
            .await?;
        if !deleted {
            return Err(Error::PostNotFound);
        }
        Ok(())
    }

    /// Public listing for a blog slug: only published, not deleted posts.
    pub async fn public_page(
        &self,
        blog_slug: &str,
        page_param: Option<&str>,
        size_param: Option<&str>,
    ) -> Result<ListResponse<PublicPostResponse>, Error> {
        let blog_id = self.public_blog_id(blog_slug).await?;
        let page = parse_page_number(page_param)?;
        let size = parse_page_size(size_param)?;
        let offset = u64::from(page.saturating_sub(1)) * u64::from(size);
        let rows = self
            .posts
            .page_published(
                &blog_id,
                i64::from(size),
                i64::try_from(offset).map_err(|_| Error::InvalidInput)?,
            )
            .await?;
        let total = self.posts.count_published(&blog_id).await?;
        Ok(ListResponse {
            items: rows.into_iter().map(PublicPostResponse::from).collect(),
            page,
            size,
            total,
        })
    }

    /// Public read of one published post by its slug.
    pub async fn public_post(
        &self,
        blog_slug: &str,
        post_slug: &str,
    ) -> Result<PublicPostResponse, Error> {
        let blog_id = self.public_blog_id(blog_slug).await?;
        let row = self
            .posts
            .find_by_slug(&blog_id, post_slug)
            .await?
            .ok_or(Error::PostNotFound)?;
        if row.status != STATUS_PUBLISHED {
            return Err(Error::PostNotFound);
        }
        Ok(PublicPostResponse::from(row))
    }

    async fn owned_blog(
        &self,
        owner_id: Uuid,
        blog_id_param: &str,
    ) -> Result<crate::blog::BlogRow, Error> {
        let blog_id = parse_uuid_v7(blog_id_param)?;
        self.blogs
            .find_for_owner(&owner_id, &blog_id)
            .await?
            .ok_or(Error::BlogNotFound)
    }

    async fn public_blog_id(&self, slug: &str) -> Result<Uuid, Error> {
        self.blogs
            .find_by_slug(slug)
            .await?
            .map(|row| row.id)
            .ok_or(Error::BlogNotFound)
    }
}

fn post_response(row: PostRow) -> PostResponse {
    PostResponse {
        id: row.id,
        blog_id: row.blog_id,
        author_id: row.author_id,
        slug: row.slug,
        title: row.title,
        content_markdown: row.content_markdown,
        status: row.status,
        created_at: row.created_at,
        updated_at: row.updated_at,
        published_at: row.published_at,
    }
}

impl From<PostRow> for PublicPostResponse {
    fn from(row: PostRow) -> PublicPostResponse {
        PublicPostResponse {
            slug: row.slug,
            title: row.title,
            content_markdown: row.content_markdown,
            published_at: row.published_at.unwrap_or_default(),
        }
    }
}

/// Publication status is constrained to the two supported states; unknown
/// values are rejected instead of mapped so callers never guess.
fn valid_status(status: Option<&str>) -> Result<&'static str, Error> {
    match status {
        None => Ok(STATUS_DRAFT),
        Some(STATUS_DRAFT) => Ok(STATUS_DRAFT),
        Some(STATUS_PUBLISHED) => Ok(STATUS_PUBLISHED),
        Some(_) => Err(Error::InvalidInput),
    }
}

/// Titles are single-line, trimmed, printable and bounded so no user can
/// exhaust storage or corrupt listings with control characters.
fn valid_title(title: &str) -> Result<String, Error> {
    let title = title.trim();
    if title.is_empty()
        || title.len() > MAX_TITLE_BYTES
        || title
            .chars()
            .any(|ch| !(0x20..=0x7E).contains(&(ch as u32)) && !(ch as u32 >= 0xA0))
    {
        return Err(Error::InvalidInput);
    }
    Ok(title.to_owned())
}

fn valid_slug_field(slug: &str) -> Result<String, Error> {
    if crate::ids::valid_slug(slug) {
        Ok(slug.to_owned())
    } else {
        Err(Error::InvalidInput)
    }
}

/// Markdown content is stored as the canonical byte representation: a
/// leading byte-order mark is stripped, line endings are normalized to LF,
/// and NUL, other C0/C1 controls and DEL are rejected outright. Leading
/// whitespace stays meaningful; only the size is bounded.
fn valid_content(content: &str) -> Result<String, Error> {
    if content.len() > MAX_CONTENT_BYTES {
        return Err(Error::InvalidInput);
    }
    let content = content.trim_start_matches('\u{FEFF}');
    let net = content.replace("\r\n", "\n").replace('\r', "\n");
    if net.len() > MAX_CONTENT_BYTES {
        return Err(Error::InvalidInput);
    }
    if net.chars().any(|ch| {
        !(0x0A == ch as u32
            || 0x09 == ch as u32
            || (0x20..=0x7E).contains(&(ch as u32))
            || ch as u32 >= 0xA0)
    }) {
        return Err(Error::InvalidInput);
    }
    Ok(net)
}
