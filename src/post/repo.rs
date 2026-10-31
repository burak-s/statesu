use sqlx::sqlite::SqlitePool;

use crate::error::{Error, InternalError};
use crate::post::{PostRow, RawPostRow, STATUS_PUBLISHED};

/// SQL-only access to the `posts` table. Orderings always include an `id`
/// tiebreak, so results stay deterministic for second-granularity timestamps.
#[derive(Clone)]
pub struct PostRepository {
    pool: SqlitePool,
}

/// Everything a new post row needs; keeps SQL call sites readable.
pub struct NewPost<'a> {
    pub id: &'a uuid::Uuid,
    pub blog_id: &'a uuid::Uuid,
    pub author_id: &'a uuid::Uuid,
    pub slug: &'a str,
    pub title: &'a str,
    pub content_markdown: &'a str,
    pub status: &'a str,
    pub now: i64,
}

impl PostRepository {
    pub fn new(pool: SqlitePool) -> PostRepository {
        PostRepository { pool }
    }

    pub async fn insert(&self, post: &NewPost<'_>) -> Result<(), Error> {
        let published_at = (post.status == STATUS_PUBLISHED).then_some(post.now);
        let result = sqlx::query(
            "INSERT INTO posts (id, blog_id, author_id, slug, title, content_markdown, \
             status, created_at, updated_at, published_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(post.id.to_string())
        .bind(post.blog_id.to_string())
        .bind(post.author_id.to_string())
        .bind(post.slug)
        .bind(post.title)
        .bind(post.content_markdown)
        .bind(post.status)
        .bind(post.now)
        .bind(post.now)
        .bind(published_at)
        .execute(&self.pool)
        .await
        .map_err(|err| insert_error(err, "post insert"))?;
        if result.rows_affected() != 1 {
            return Err(failed("post insert affected no row"));
        }
        Ok(())
    }

    /// One management page for one blog, newest first.
    pub async fn page_for_blog(
        &self,
        blog_id: &uuid::Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PostRow>, Error> {
        let rows = sqlx::query_as::<_, RawPostRow>(
            "SELECT id, blog_id, author_id, slug, title, content_markdown, status, \
             created_at, updated_at, published_at, deleted_at \
             FROM posts WHERE blog_id = ? AND deleted_at IS NULL \
             ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(blog_id.to_string())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| failed("post page query"))?;
        rows.into_iter().map(RawPostRow::into_post_row).collect()
    }

    pub async fn count_for_blog(&self, blog_id: &uuid::Uuid) -> Result<i64, Error> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM posts WHERE blog_id = ? AND deleted_at IS NULL",
        )
        .bind(blog_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|_| failed("post count"))
    }

    pub async fn find(
        &self,
        blog_id: &uuid::Uuid,
        post_id: &uuid::Uuid,
    ) -> Result<Option<PostRow>, Error> {
        sqlx::query_as::<_, RawPostRow>(
            "SELECT id, blog_id, author_id, slug, title, content_markdown, status, \
             created_at, updated_at, published_at, deleted_at \
             FROM posts WHERE blog_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(blog_id.to_string())
        .bind(post_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| failed("post find"))?
        .map(RawPostRow::into_post_row)
        .transpose()
    }

    pub async fn find_by_slug(
        &self,
        blog_id: &uuid::Uuid,
        slug: &str,
    ) -> Result<Option<PostRow>, Error> {
        sqlx::query_as::<_, RawPostRow>(
            "SELECT id, blog_id, author_id, slug, title, content_markdown, status, \
             created_at, updated_at, published_at, deleted_at \
             FROM posts WHERE blog_id = ? AND slug = ? AND deleted_at IS NULL",
        )
        .bind(blog_id.to_string())
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| failed("post find by slug"))?
        .map(RawPostRow::into_post_row)
        .transpose()
    }

    /// Full-row update for patches. Timestamp and publication fields are
    /// decided by the service; ownership stays in the blog lookup.
    pub async fn update(&self, post: &PostRow) -> Result<(), Error> {
        let result = sqlx::query(
            "UPDATE posts SET slug = ?, title = ?, content_markdown = ?, status = ?, \
             updated_at = ?, published_at = ? WHERE id = ? AND blog_id = ? AND deleted_at IS NULL",
        )
        .bind(post.slug.as_str())
        .bind(post.title.as_str())
        .bind(post.content_markdown.as_str())
        .bind(post.status.as_str())
        .bind(post.updated_at)
        .bind(post.published_at)
        .bind(post.id.to_string())
        .bind(post.blog_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|err| insert_error(err, "post update"))?;
        if result.rows_affected() != 1 {
            return Err(failed("post update affected no row"));
        }
        Ok(())
    }

    /// Soft delete: the slug stays reserved but every read excludes the row.
    pub async fn soft_delete(
        &self,
        blog_id: &uuid::Uuid,
        post_id: &uuid::Uuid,
        deleted_at: i64,
    ) -> Result<bool, Error> {
        let result = sqlx::query(
            "UPDATE posts SET deleted_at = ?, updated_at = ? \
             WHERE blog_id = ? AND id = ? AND deleted_at IS NULL",
        )
        .bind(deleted_at)
        .bind(deleted_at)
        .bind(blog_id.to_string())
        .bind(post_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_| failed("post delete"))?;
        Ok(result.rows_affected() == 1)
    }

    /// Public page: drafts and deleted posts never appear.
    pub async fn page_published(
        &self,
        blog_id: &uuid::Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<PostRow>, Error> {
        let rows = sqlx::query_as::<_, RawPostRow>(
            "SELECT id, blog_id, author_id, slug, title, content_markdown, status, \
             created_at, updated_at, published_at, deleted_at \
             FROM posts WHERE blog_id = ? AND deleted_at IS NULL AND status = ? \
             ORDER BY published_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(blog_id.to_string())
        .bind(STATUS_PUBLISHED)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| failed("public post page query"))?;
        rows.into_iter().map(RawPostRow::into_post_row).collect()
    }

    pub async fn count_published(&self, blog_id: &uuid::Uuid) -> Result<i64, Error> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM posts \
             WHERE blog_id = ? AND deleted_at IS NULL AND status = ?",
        )
        .bind(blog_id.to_string())
        .bind(STATUS_PUBLISHED)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| failed("public post count"))
    }
}

/// Unique violations map to a stable slug conflict; anything else stays
/// internal.
fn insert_error(err: sqlx::Error, context: &'static str) -> Error {
    // SQLite reports the (blog_id, slug) unique index as a column list, so the
    // check runs on the message text instead of a constraint name.
    if err
        .as_database_error()
        .is_some_and(|database| database.message().contains("posts.slug"))
    {
        return Error::SlugTaken;
    }
    Error::Internal(InternalError::new(std::io::Error::other(format!(
        "{context}: {err}"
    ))))
}

fn failed(message: &'static str) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(message)))
}
