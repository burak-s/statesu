pub mod repo;
pub mod service;

use uuid::Uuid;

use crate::error::Error;
use crate::ids::parse_uuid_v7;

/// Post publication status; drafts are management-only until published.
pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_PUBLISHED: &str = "published";

/// Storage shape for the `posts` table.
#[derive(Debug, Clone)]
pub struct PostRow {
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
    pub deleted_at: Option<i64>,
}

/// `sqlx::FromRow` projection of the `posts` table before ID validation.
#[derive(Debug, sqlx::FromRow)]
pub struct RawPostRow {
    id: String,
    blog_id: String,
    author_id: String,
    slug: String,
    title: String,
    content_markdown: String,
    status: String,
    created_at: i64,
    updated_at: i64,
    published_at: Option<i64>,
    deleted_at: Option<i64>,
}

impl RawPostRow {
    pub fn into_post_row(self) -> Result<PostRow, Error> {
        Ok(PostRow {
            id: parse_uuid_v7(&self.id)?,
            blog_id: parse_uuid_v7(&self.blog_id)?,
            author_id: parse_uuid_v7(&self.author_id)?,
            slug: self.slug,
            title: self.title,
            content_markdown: self.content_markdown,
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            published_at: self.published_at,
            deleted_at: self.deleted_at,
        })
    }
}
