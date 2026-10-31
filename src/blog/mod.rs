pub mod repo;
pub mod service;

use uuid::Uuid;

use crate::error::Error;
use crate::ids::parse_uuid_v7;

/// Storage shape for the `blogs` table.
#[derive(Debug, Clone)]
pub struct BlogRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub slug: String,
    pub domain: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// `sqlx::FromRow` projection of the `blogs` table before ID validation.
#[derive(Debug, sqlx::FromRow)]
pub struct RawBlogRow {
    id: String,
    owner_id: String,
    name: String,
    slug: String,
    domain: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl RawBlogRow {
    pub fn into_blog_row(self) -> Result<BlogRow, Error> {
        Ok(BlogRow {
            id: parse_uuid_v7(&self.id)?,
            owner_id: parse_uuid_v7(&self.owner_id)?,
            name: self.name,
            slug: self.slug,
            domain: self.domain,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}
