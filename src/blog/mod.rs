pub mod appearance;
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
    /// Raw saved embed appearance JSON (validated at write time);
    /// `None` means "use the built-in appearance".
    pub embed_appearance: Option<String>,
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
    embed_appearance: Option<String>,
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
            embed_appearance: self.embed_appearance,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Storage shape for a named appearance variant (`blog_appearance_variants`).
/// `appearance` is validated JSON, the same document shape the blog row
/// stores.
#[derive(Debug, Clone)]
pub struct AppearanceVariantRow {
    pub id: Uuid,
    pub blog_id: Uuid,
    pub name: String,
    pub appearance: String,
    pub created_at: i64,
}

/// `sqlx::FromRow` projection of the variant table before ID validation.
#[derive(Debug, sqlx::FromRow)]
pub struct RawVariantRow {
    id: String,
    blog_id: String,
    name: String,
    appearance: String,
    created_at: i64,
}

impl RawVariantRow {
    pub fn into_variant_row(self) -> Result<AppearanceVariantRow, Error> {
        Ok(AppearanceVariantRow {
            id: parse_uuid_v7(&self.id)?,
            blog_id: parse_uuid_v7(&self.blog_id)?,
            name: self.name,
            appearance: self.appearance,
            created_at: self.created_at,
        })
    }
}
