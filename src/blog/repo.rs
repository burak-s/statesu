use sqlx::sqlite::SqlitePool;

use crate::blog::{BlogRow, RawBlogRow};
use crate::error::{Error, InternalError};

/// SQL-only access to the `blogs` and `blog_origins` tables. Orderings always
/// include an `id` tiebreak, so results stay deterministic for
/// second-granularity timestamps.
#[derive(Clone)]
pub struct BlogRepository {
    pool: SqlitePool,
}

impl BlogRepository {
    pub fn new(pool: SqlitePool) -> BlogRepository {
        BlogRepository { pool }
    }

    pub async fn insert(
        &self,
        id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
        name: &str,
        slug: &str,
        domain: Option<&str>,
        created_at: i64,
    ) -> Result<(), Error> {
        let result = sqlx::query(
            "INSERT INTO blogs (id, owner_id, name, slug, domain, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(owner_id.to_string())
        .bind(name)
        .bind(slug)
        .bind(domain)
        .bind(created_at)
        .bind(created_at)
        .execute(&self.pool)
        .await
        .map_err(|err| insert_error(err, "blog insert"))?;
        if result.rows_affected() != 1 {
            return Err(failed("blog insert affected no row"));
        }
        Ok(())
    }

    pub async fn find_by_slug(&self, slug: &str) -> Result<Option<BlogRow>, Error> {
        sqlx::query_as::<_, RawBlogRow>(
            "SELECT id, owner_id, name, slug, domain, embed_appearance, created_at, updated_at \
             FROM blogs WHERE slug = ?",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| failed("blog find by slug"))?
        .map(RawBlogRow::into_blog_row)
        .transpose()
    }

    pub async fn count_for_owner(&self, owner_id: &uuid::Uuid) -> Result<i64, Error> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM blogs WHERE owner_id = ?")
            .bind(owner_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(|_| failed("blog count"))
    }

    /// Ownership is part of the SQL predicate; unknown and foreign blogs read
    /// as `None` so both map to the same 404.
    pub async fn find_for_owner(
        &self,
        owner_id: &uuid::Uuid,
        id: &uuid::Uuid,
    ) -> Result<Option<BlogRow>, Error> {
        sqlx::query_as::<_, RawBlogRow>(
            "SELECT id, owner_id, name, slug, domain, embed_appearance, created_at, updated_at \
             FROM blogs WHERE id = ? AND owner_id = ?",
        )
        .bind(id.to_string())
        .bind(owner_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| failed("blog find for owner"))?
        .map(RawBlogRow::into_blog_row)
        .transpose()
    }

    pub async fn list_for_owner(&self, owner_id: &uuid::Uuid) -> Result<Vec<BlogRow>, Error> {
        let rows = sqlx::query_as::<_, RawBlogRow>(
            "SELECT id, owner_id, name, slug, domain, embed_appearance, created_at, updated_at \
             FROM blogs WHERE owner_id = ? ORDER BY created_at DESC, id DESC",
        )
        .bind(owner_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| failed("blog list query"))?;
        rows.into_iter().map(RawBlogRow::into_blog_row).collect()
    }

    /// Exact browser origins recorded for the blog with the given slug;
    /// unknown blogs and blogs without origins both read as an empty list.
    pub async fn origins_for_slug(&self, slug: &str) -> Result<Vec<String>, Error> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT o.origin \
             FROM blog_origins o JOIN blogs b ON b.id = o.blog_id \
             WHERE b.slug = ? ORDER BY o.origin",
        )
        .bind(slug)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| failed("blog origins query"))?;
        Ok(rows.into_iter().map(|(origin,)| origin).collect())
    }

    /// Ownership is part of the SQL predicate; zero affected rows means the
    /// blog is missing or foreign, both mapped to the same 404.
    pub async fn update(
        &self,
        id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
        name: &str,
        domain: Option<&str>,
        updated_at: i64,
    ) -> Result<(), Error> {
        let result = sqlx::query(
            "UPDATE blogs SET name = ?, domain = ?, updated_at = ? \
             WHERE id = ? AND owner_id = ?",
        )
        .bind(name)
        .bind(domain)
        .bind(updated_at)
        .bind(id.to_string())
        .bind(owner_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|err| insert_error(err, "blog update"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        Ok(())
    }

    /// Replaces the saved embed appearance (JSON) or clears it; ownership is
    /// part of the SQL predicate like every other blog mutation.
    pub async fn update_appearance(
        &self,
        id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
        appearance: Option<&str>,
        updated_at: i64,
    ) -> Result<(), Error> {
        let result = sqlx::query(
            "UPDATE blogs SET embed_appearance = ?, updated_at = ? \
             WHERE id = ? AND owner_id = ?",
        )
        .bind(appearance)
        .bind(updated_at)
        .bind(id.to_string())
        .bind(owner_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_| failed("blog appearance update"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        Ok(())
    }

    /// Deleting a blog cascades to its posts and allowed origins.
    pub async fn delete(&self, id: &uuid::Uuid, owner_id: &uuid::Uuid) -> Result<(), Error> {
        let result = sqlx::query("DELETE FROM blogs WHERE id = ? AND owner_id = ?")
            .bind(id.to_string())
            .bind(owner_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|_| failed("blog delete"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        Ok(())
    }

    /// Deleting a blog together with its durable media-cleanup jobs, in
    /// one transaction: either the rows vanish and the bucket cleanup is
    /// durable, or nothing happened — never a deleted blog whose media
    /// objects went untracked. `expected_fingerprints` guard against any
    /// media mutation racing a snapshot built outside the transaction
    /// (ids, statuses and variant paths all compared; an upload that
    /// finalizes between snapshot and delete keeps its id but changes
    /// its paths); mismatch returns a retryable conflict before deleting
    /// anything.
    pub async fn delete_with_media_cleanup(
        &self,
        id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
        expected_fingerprints: &[crate::media::media_repo::MediaFingerprint],
        jobs: &[crate::media::media_repo::MediaJobInsert],
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| failed("blog delete transaction"))?;
        let mut expected = crate::media::media_repo::MediaRepository::new(self.pool.clone())
            .blog_media_fingerprints_tx(&mut tx, owner_id, id)
            .await?;
        let mut snapshot: Vec<crate::media::media_repo::MediaFingerprint> =
            expected_fingerprints.to_vec();
        expected.sort_by_key(|item| item.media_id);
        snapshot.sort_by_key(|item| item.media_id);
        if expected != snapshot {
            return Err(Error::RetrySnapshot);
        }
        let result = sqlx::query("DELETE FROM blogs WHERE id = ? AND owner_id = ?")
            .bind(id.to_string())
            .bind(owner_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_| failed("blog delete"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        for job in jobs {
            let result = sqlx::query(
                "INSERT INTO media_jobs (id, owner_id, media_id, blog_id, kind, payload, \
                 attempts, next_run_at, created_at) \
                 VALUES (?, ?, NULL, ?, ?, ?, 0, ?, ?)",
            )
            .bind(job.id.to_string())
            .bind(job.owner_id.to_string())
            .bind(job.blog_id.map(|id| id.to_string()))
            .bind(job.kind)
            .bind(&job.payload)
            .bind(job.next_run_at)
            .bind(job.created_at)
            .execute(&mut *tx)
            .await
            .map_err(|_| failed("media job insert"))?;
            if result.rows_affected() != 1 {
                return Err(failed("media job insert affected no row"));
            }
        }
        tx.commit()
            .await
            .map_err(|_| failed("blog delete commit"))?;
        Ok(())
    }

    /// Replaces the whole origin list atomically.
    pub async fn replace_origins(
        &self,
        blog_id: &uuid::Uuid,
        origins: Vec<String>,
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| failed("origin transaction start"))?;
        sqlx::query("DELETE FROM blog_origins WHERE blog_id = ?")
            .bind(blog_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_| failed("origin delete"))?;
        for origin in origins {
            sqlx::query("INSERT INTO blog_origins (blog_id, origin) VALUES (?, ?)")
                .bind(blog_id.to_string())
                .bind(origin)
                .execute(&mut *tx)
                .await
                .map_err(|_| failed("origin insert"))?;
        }
        tx.commit()
            .await
            .map_err(|_| failed("origin transaction commit"))
    }
}

/// Unique violations map to stable conflict errors; anything else stays
/// internal.
fn insert_error(err: sqlx::Error, context: &'static str) -> Error {
    let columns = err
        .as_database_error()
        .map(|database| database.message())
        .unwrap_or_default();
    match columns {
        columns if columns.contains("blogs.slug") => Error::SlugTaken,
        columns if columns.contains("blogs.domain") => Error::DomainTaken,
        _ => Error::Internal(InternalError::new(std::io::Error::other(format!(
            "{context}: {err}"
        )))),
    }
}

fn failed(message: &'static str) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(message)))
}
