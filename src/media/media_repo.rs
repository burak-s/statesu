//! SQL access to the `media`, `media_variants` and `media_jobs` tables.
//!
//! Quota reservation, finalization and cleanup-job transitions all happen
//! in single statements or single transactions so concurrent requests and
//! background maintenance cannot interleave half-done state.

use sqlx::sqlite::SqlitePool;

use crate::error::Error;
use crate::ids::parse_uuid_v7;
use crate::media::repo::internal;

/// Columns of `media` every row-select shares.
const MEDIA_COLUMNS: &str = "id, blog_id, owner_id, connection_id, public_base_url, storage_location, \
     status, mime, width, height, stored_bytes, reserved_bytes, created_at, updated_at";

#[derive(Clone)]
pub struct MediaRepository {
    pool: SqlitePool,
}

impl MediaRepository {
    pub fn new(pool: SqlitePool) -> MediaRepository {
        MediaRepository { pool }
    }

    /// Atomically reserves one media record. The row is inserted only when
    /// the per-user record cap holds; the byte budget applies to the
    /// server's own bucket exclusively (custom buckets hold the user's
    /// own bytes, so Statesu's quota does not apply there).
    pub async fn reserve(
        &self,
        insert: &MediaInsert,
        max_total_bytes: i64,
        max_media_per_user: i64,
    ) -> Result<(), Error> {
        // The byte budget applies to the server's own bucket only;
        // custom buckets hold the user's own bytes.
        let skip_byte_check = insert.connection_id.is_some();
        let result = sqlx::query(
            "INSERT INTO media (id, blog_id, owner_id, connection_id, public_base_url, status, \
             mime, width, height, stored_bytes, reserved_bytes, created_at, updated_at) \
             SELECT ?, ?, ?, ?, ?, 'uploading', ?, NULL, NULL, 0, ?, ?, ? \
             WHERE (SELECT COUNT(*) FROM media WHERE owner_id = ?) < ? \
             AND (? OR (SELECT COALESCE(SUM(stored_bytes + reserved_bytes), 0) FROM media \
                          WHERE owner_id = ? AND connection_id IS NULL) + ? <= ?)",
        )
        .bind(insert.id.to_string())
        .bind(insert.blog_id.to_string())
        .bind(insert.owner_id.to_string())
        .bind(insert.connection_id.map(|id| id.to_string()))
        .bind(&insert.public_base_url)
        .bind(&insert.mime)
        .bind(insert.reserved_bytes)
        .bind(insert.now)
        .bind(insert.now)
        .bind(insert.owner_id.to_string())
        .bind(max_media_per_user)
        .bind(skip_byte_check)
        .bind(insert.owner_id.to_string())
        .bind(insert.reserved_bytes)
        .bind(max_total_bytes)
        .execute(&self.pool)
        .await
        .map_err(|_err| internal("media reserve"))?;
        if result.rows_affected() == 0 {
            return Err(Error::Forbidden);
        }
        Ok(())
    }

    /// Commits `ready` plus every variant in one transaction, re-checked
    /// against the byte budget when the row sits on the server bucket.
    /// Either both exist or neither: no half-published records.
    pub async fn finalize_ready_with_variants(
        &self,
        finalize: &Finalize<'_>,
        variants: &[MediaVariantInsert],
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_err| internal("media finalize begin"))?;
        let result = sqlx::query(
            "UPDATE media SET status = 'ready', mime = ?, width = ?, height = ?, \
             stored_bytes = ?, reserved_bytes = 0, updated_at = ?, \
             storage_location = ? \
             WHERE id = ? AND owner_id = ? AND status = 'uploading' \
             AND (? OR (SELECT COALESCE(SUM(stored_bytes + reserved_bytes), 0) FROM media \
                          WHERE owner_id = ? AND connection_id IS NULL) - reserved_bytes + ? <= ?)",
        )
        .bind(finalize.mime)
        .bind(i64::from(finalize.width))
        .bind(i64::from(finalize.height))
        .bind(finalize.stored_bytes)
        .bind(finalize.now)
        .bind(finalize.storage_location)
        .bind(finalize.id.to_string())
        .bind(finalize.owner_id.to_string())
        .bind(finalize.exempt_from_quota)
        .bind(finalize.owner_id.to_string())
        .bind(finalize.stored_bytes)
        .bind(finalize.max_total_bytes)
        .execute(&mut *tx)
        .await
        .map_err(|_err| internal("media finalize"))?;
        if result.rows_affected() != 1 {
            return Err(Error::Forbidden);
        }
        for row in variants {
            let result = sqlx::query(
                "INSERT INTO media_variants (media_id, label, object_key, mime, width, height, bytes) \
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(row.media_id.to_string())
            .bind(&row.label)
            .bind(&row.object_key)
            .bind(&row.mime)
            .bind(row.width.map(i64::from))
            .bind(row.height.map(i64::from))
            .bind(row.bytes)
            .execute(&mut *tx)
            .await
            .map_err(|_err| internal("media variant insert"))?;
            if result.rows_affected() != 1 {
                return Err(internal("media variant insert affected no row"));
            }
        }
        tx.commit()
            .await
            .map_err(|_err| internal("media finalize commit"))?;
        Ok(())
    }

    /// A failed upload keeps a record for visibility but no longer holds
    /// quota bytes.
    pub async fn release(&self, id: &uuid::Uuid, now: i64) -> Result<(), Error> {
        sqlx::query(
            "UPDATE media SET status = 'error', stored_bytes = 0, reserved_bytes = 0, \
             updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_err| internal("media release"))?;
        Ok(())
    }

    pub async fn variants_for(&self, media_id: &uuid::Uuid) -> Result<Vec<MediaVariantRow>, Error> {
        let rows = sqlx::query_as::<_, RawVariantRow>(
            "SELECT media_id, label, object_key, mime, width, height, bytes \
             FROM media_variants WHERE media_id = ? ORDER BY label",
        )
        .bind(media_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| internal("media variants query"))?;
        rows.into_iter().map(RawVariantRow::into_row).collect()
    }

    pub async fn find_for_owner(
        &self,
        owner_id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
        media_id: &uuid::Uuid,
    ) -> Result<Option<MediaRow>, Error> {
        sqlx::query_as::<_, RawMediaRow>(
            "SELECT id, blog_id, owner_id, connection_id, public_base_url, storage_location, status, mime, width, \
             height, stored_bytes, reserved_bytes, created_at, updated_at \
             FROM media WHERE id = ? AND blog_id = ? AND owner_id = ?",
        )
        .bind(media_id.to_string())
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| internal("media find"))?
        .map(RawMediaRow::into_row)
        .transpose()
    }

    /// Public delivery source: ready media rows for one blog with the
    /// publishing base snapshotted at commit time. Bounded chunking keeps
    /// SQLite's bind budget safe even for Markdown with many references.
    pub async fn public_ready_with_base(
        &self,
        blog_id: &uuid::Uuid,
        ids: &[uuid::Uuid],
    ) -> Result<Vec<PublicMediaSource>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut sources = Vec::new();
        for (chunk_index, chunk) in ids.chunks(200).enumerate() {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT m.id, m.mime, m.width, m.height, m.public_base_url \
                 FROM media m WHERE m.blog_id = ? AND m.status = 'ready' AND m.id IN ({placeholders})"
            );
            // Only generated `?` placeholders are interpolated; all values are bound.
            let mut query =
                sqlx::query_as::<_, RawPublicMediaSource>(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(blog_id.to_string());
            for id in chunk {
                query = query.bind(id.to_string());
            }
            let _ = chunk_index;
            for raw in query
                .fetch_all(&self.pool)
                .await
                .map_err(|_| internal("public media query"))?
            {
                sources.push(raw.into_row()?);
            }
        }
        Ok(sources)
    }

    /// Variant rows for a set of media ids; chunked like the source scan.
    pub async fn variants_for_many(
        &self,
        ids: &[uuid::Uuid],
    ) -> Result<Vec<MediaVariantRow>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows = Vec::new();
        for chunk in ids.chunks(200) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT media_id, label, object_key, mime, width, height, bytes \
                 FROM media_variants WHERE media_id IN ({placeholders}) ORDER BY label"
            );
            // Only generated `?` placeholders are interpolated; all values are bound.
            let mut query = sqlx::query_as::<_, RawVariantRow>(sqlx::AssertSqlSafe(sql.as_str()));
            for id in chunk {
                query = query.bind(id.to_string());
            }
            for raw in query
                .fetch_all(&self.pool)
                .await
                .map_err(|_| internal("public media variants query"))?
            {
                rows.push(raw.into_row()?);
            }
        }
        Ok(rows)
    }

    pub async fn page_for_blog(
        &self,
        blog_id: &uuid::Uuid,
        size: i64,
        offset: i64,
    ) -> Result<Vec<MediaRow>, Error> {
        // Deleting rows are hidden: the user's intent is final; the
        // background job only cleans up after them.
        let rows = sqlx::query_as::<_, RawMediaRow>(
            "SELECT id, blog_id, owner_id, connection_id, public_base_url, storage_location, status, mime, width, \
             height, stored_bytes, reserved_bytes, created_at, updated_at \
             FROM media WHERE blog_id = ? AND status != 'deleting' \
             ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(blog_id.to_string())
        .bind(size)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| internal("media page"))?;
        rows.into_iter().map(RawMediaRow::into_row).collect()
    }

    pub async fn count_for_blog(&self, blog_id: &uuid::Uuid) -> Result<i64, Error> {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM media WHERE blog_id = ? AND status != 'deleting'",
        )
        .bind(blog_id.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|_| internal("media count"))
    }

    /// Removes the record cascade; variants follow by FK. Only a deleting
    /// (or failed) row may vanish this way, so in-flight match failures
    /// stay loud.
    pub async fn delete_row(
        &self,
        id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
    ) -> Result<(), Error> {
        let result = sqlx::query("DELETE FROM media WHERE id = ? AND blog_id = ? AND owner_id = ?")
            .bind(id.to_string())
            .bind(blog_id.to_string())
            .bind(owner_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|_| internal("media delete"))?;
        if result.rows_affected() != 1 {
            return Err(internal("media delete affected no row"));
        }
        Ok(())
    }

    /// Marks an owned media row for deletion; public queries and lists
    /// exclude it from that exact moment on. Objects still exist until the
    /// durable job reports them gone.
    pub async fn mark_for_deletion(
        &self,
        id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
    ) -> Result<(), Error> {
        let result = sqlx::query(
            "UPDATE media SET status = 'deleting', updated_at = ? \
             WHERE id = ? AND blog_id = ? AND owner_id = ?",
        )
        .bind(now_seconds())
        .bind(id.to_string())
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_err| internal("media mark deletion"))?;
        if result.rows_affected() != 1 {
            return Err(internal("media mark deletion affected no row"));
        }
        Ok(())
    }

    /// Removes the media row and its finished job together; either both or
    /// neither, so a job can never outlive its row's commit.
    pub async fn complete_deletion(
        &self,
        media_id: &uuid::Uuid,
        job_id: &uuid::Uuid,
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_err| internal("media deletion begin"))?;
        let result = sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(media_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_err| internal("media deletion row"))?;
        sqlx::query("DELETE FROM media_jobs WHERE id = ?")
            .bind(job_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_err| internal("media deletion job"))?;
        tx.commit()
            .await
            .map_err(|_err| internal("media deletion commit"))?;
        let _ = result;
        Ok(())
    }

    /// Registers one durable cleanup job for the object paths in payload.
    pub async fn insert_job(&self, insert: &MediaJobInsert) -> Result<(), Error> {
        let result = sqlx::query(
            "INSERT INTO media_jobs (id, owner_id, media_id, blog_id, kind, payload, \
             attempts, next_run_at, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(insert.id.to_string())
        .bind(insert.owner_id.to_string())
        .bind(insert.media_id.map(|id| id.to_string()))
        .bind(insert.blog_id.map(|id| id.to_string()))
        .bind(insert.kind)
        .bind(&insert.payload)
        .bind(insert.next_run_at)
        .bind(insert.created_at)
        .execute(&self.pool)
        .await
        .map_err(|_err| internal("media job insert"))?;
        if result.rows_affected() != 1 {
            return Err(internal("media job insert affected no row"));
        }
        Ok(())
    }

    /// Jobs due for one retry window, oldest attempt first.
    pub async fn due_jobs(&self, now: i64, max_attempts: i64) -> Result<Vec<MediaJobRow>, Error> {
        let rows = sqlx::query_as::<_, RawJobRow>(
            "SELECT id, owner_id, media_id, blog_id, kind, payload, attempts, next_run_at, \
             last_error, created_at FROM media_jobs \
             WHERE next_run_at <= ? AND attempts < ? ORDER BY next_run_at, id LIMIT 100",
        )
        .bind(now)
        .bind(max_attempts)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| internal("media jobs due"))?;
        rows.into_iter().map(RawJobRow::into_row).collect()
    }

    pub async fn delete_job(&self, id: &uuid::Uuid) -> Result<(), Error> {
        sqlx::query("DELETE FROM media_jobs WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|_err| internal("media job delete"))?;
        Ok(())
    }

    /// Configuration-wait bookkeeping: like `schedule_retry`, but the
    /// retry budget stays untouched (the payload carries its own wait
    /// counter) and the payload itself is refreshed.
    pub async fn schedule_wait(
        &self,
        id: &uuid::Uuid,
        payload: &str,
        next_run_at: i64,
        last_error: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            "UPDATE media_jobs SET payload = ?, next_run_at = ?, last_error = ? WHERE id = ?",
        )
        .bind(payload)
        .bind(next_run_at)
        .bind(last_error)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_err| internal("media job wait schedule"))?;
        Ok(())
    }

    pub async fn schedule_retry(
        &self,
        id: &uuid::Uuid,
        attempts: i64,
        next_run_at: i64,
        last_error: &str,
    ) -> Result<(), Error> {
        sqlx::query(
            "UPDATE media_jobs SET attempts = ?, next_run_at = ?, last_error = ? WHERE id = ?",
        )
        .bind(attempts)
        .bind(next_run_at)
        .bind(last_error)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_err| internal("media job retry"))?;
        Ok(())
    }

    /// Uploaded-but-not-finalized records older than the cutoff: their
    /// request died mid-flight and their objects may still exist. Their
    /// cleanup job was registered before any object write, so only the
    /// row release is needed here.
    pub async fn stale_uploading(&self, cutoff: i64, limit: i64) -> Result<Vec<MediaRow>, Error> {
        let rows = sqlx::query_as::<_, RawMediaRow>(
            "SELECT id, blog_id, owner_id, connection_id, public_base_url, storage_location, status, mime, width, \
             height, stored_bytes, reserved_bytes, created_at, updated_at \
             FROM media WHERE status = 'uploading' AND created_at <= ? LIMIT ?",
        )
        .bind(cutoff)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|_| internal("media stale scan"))?;
        rows.into_iter().map(RawMediaRow::into_row).collect()
    }

    /// Current status of one row; `None` when the row no longer exists.
    /// Maintenance reads this to avoid chopping objects out from under a
    /// record that recovered.
    pub async fn status_of(&self, media_id: &uuid::Uuid) -> Result<Option<String>, Error> {
        sqlx::query_scalar::<_, String>("SELECT status FROM media WHERE id = ?")
            .bind(media_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| internal("media status"))
    }

    /// Cleanup inputs for one media row: theJobs variant rows (their
    /// object keys are absolute) and the row's own connection reference.
    /// Blog deletion's caller takes the snapshot before the cascade.
    pub async fn blog_media_cleanup(
        &self,
        owner_id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
    ) -> Result<Vec<MediaRow>, Error> {
        let rows = sqlx::query_as::<_, RawMediaRow>(
            "SELECT id, blog_id, owner_id, connection_id, public_base_url, storage_location, status, mime, width, \
             height, stored_bytes, reserved_bytes, created_at, updated_at \
             FROM media WHERE blog_id = ? AND owner_id = ?",
        )
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| internal("media blog cleanup"))?;
        rows.into_iter().map(RawMediaRow::into_row).collect()
    }
}

fn now_seconds() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_secs()).unwrap_or(0)
}

/// One finalization attempt: real processed bytes committed atomically
/// against the budget the reservation used.
pub struct Finalize<'a> {
    pub id: &'a uuid::Uuid,
    pub owner_id: &'a uuid::Uuid,
    pub mime: &'a str,
    pub width: u32,
    pub height: u32,
    pub stored_bytes: i64,
    pub max_total_bytes: i64,
    /// Custom-bucket records carry no Statesu byte quota; the server's
    /// bucket always does. `true` skips the byte re-check.
    pub exempt_from_quota: bool,
    /// Sealed connection material for server-bucket rows, persisted with
    /// the ready transition; `None` for custom-connection rows.
    pub storage_location: Option<&'a [u8]>,
    pub now: i64,
}

/// The reserve-side insert shape.
pub struct MediaInsert {
    pub id: uuid::Uuid,
    pub blog_id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    /// `None` means the server-configured bucket.
    pub connection_id: Option<uuid::Uuid>,
    /// Publishing base snapshotted at commit time.
    pub public_base_url: String,
    pub mime: String,
    pub reserved_bytes: i64,
    pub now: i64,
}

/// Storage shape for `media`.
#[derive(Debug, Clone)]
pub struct MediaRow {
    pub id: uuid::Uuid,
    pub blog_id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    pub connection_id: Option<uuid::Uuid>,
    /// Snapshot of the publishing base from commit time.
    pub public_base_url: String,
    /// Server-bucket location snapshot; `None` for custom rows.
    pub storage_location: Option<Vec<u8>>,
    pub status: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stored_bytes: i64,
    pub reserved_bytes: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One variant insert (object keys are absolute: connection prefix included).
pub struct MediaVariantInsert {
    pub media_id: uuid::Uuid,
    pub label: String,
    pub object_key: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: i64,
}

/// Storage shape for `media_variants`.
#[derive(Debug, Clone)]
pub struct MediaVariantRow {
    pub media_id: uuid::Uuid,
    pub label: String,
    pub object_key: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: i64,
}

/// One durable job insert; `payload` is the JSON snapshot cleanup needs.
pub struct MediaJobInsert {
    pub id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    pub media_id: Option<uuid::Uuid>,
    pub blog_id: Option<uuid::Uuid>,
    pub kind: &'static str,
    pub payload: String,
    pub next_run_at: i64,
    pub created_at: i64,
}

/// Storage shape for `media_jobs`.
#[derive(Debug, Clone)]
pub struct MediaJobRow {
    pub id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    pub media_id: Option<uuid::Uuid>,
    pub kind: String,
    pub payload: String,
    pub attempts: i64,
}

/// Per-media cleanup fingerprint: the row plus the exact variant paths
/// the cleanup payload was built from. Blog deletion's durable guard
/// re-reads this inside its transaction, so an upload that finalizes
/// between snapshot and abort instead of leaking objects under a stale
/// path list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFingerprint {
    pub media_id: uuid::Uuid,
    pub status: String,
    pub object_keys: Vec<String>,
}

/// Public delivery source for one media record plus its publishing base.
#[derive(Debug, Clone)]
pub struct PublicMediaSource {
    pub id: uuid::Uuid,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub public_base_url: String,
}

#[derive(Debug, sqlx::FromRow)]
struct RawPublicMediaSource {
    id: String,
    mime: String,
    width: Option<i64>,
    height: Option<i64>,
    public_base_url: String,
}

impl RawPublicMediaSource {
    fn into_row(self) -> Result<PublicMediaSource, Error> {
        Ok(PublicMediaSource {
            id: parse_uuid_v7(&self.id)?,
            mime: self.mime,
            width: self.width.map(|value| u32::try_from(value).unwrap_or(0)),
            height: self.height.map(|value| u32::try_from(value).unwrap_or(0)),
            public_base_url: self.public_base_url,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct RawMediaRow {
    id: String,
    blog_id: String,
    owner_id: String,
    connection_id: Option<String>,
    public_base_url: String,
    storage_location: Option<Vec<u8>>,
    status: String,
    mime: String,
    width: Option<i64>,
    height: Option<i64>,
    stored_bytes: i64,
    reserved_bytes: i64,
    created_at: i64,
    updated_at: i64,
}

impl RawMediaRow {
    fn into_row(self) -> Result<MediaRow, Error> {
        let _ = MEDIA_COLUMNS;
        Ok(MediaRow {
            id: parse_uuid_v7(&self.id)?,
            blog_id: parse_uuid_v7(&self.blog_id)?,
            owner_id: parse_uuid_v7(&self.owner_id)?,
            connection_id: self
                .connection_id
                .map(|raw| parse_uuid_v7(&raw))
                .transpose()?,
            public_base_url: self.public_base_url,
            storage_location: self.storage_location,
            status: self.status,
            mime: self.mime,
            width: self.width.map(|value| u32::try_from(value).unwrap_or(0)),
            height: self.height.map(|value| u32::try_from(value).unwrap_or(0)),
            stored_bytes: self.stored_bytes,
            reserved_bytes: self.reserved_bytes,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct RawVariantRow {
    media_id: String,
    label: String,
    object_key: String,
    mime: String,
    width: Option<i64>,
    height: Option<i64>,
    bytes: i64,
}

impl RawVariantRow {
    fn into_row(self) -> Result<MediaVariantRow, Error> {
        Ok(MediaVariantRow {
            media_id: parse_uuid_v7(&self.media_id)?,
            label: self.label,
            object_key: self.object_key,
            mime: self.mime,
            width: self.width.map(|value| u32::try_from(value).unwrap_or(0)),
            height: self.height.map(|value| u32::try_from(value).unwrap_or(0)),
            bytes: self.bytes,
        })
    }
}

#[derive(Debug, sqlx::FromRow)]
struct RawJobRow {
    id: String,
    owner_id: String,
    media_id: Option<String>,
    #[allow(dead_code)]
    blog_id: Option<String>,
    kind: String,
    payload: String,
    attempts: i64,
    #[allow(dead_code)]
    next_run_at: i64,
    #[allow(dead_code)]
    last_error: Option<String>,
    #[allow(dead_code)]
    created_at: i64,
}

impl RawJobRow {
    fn into_row(self) -> Result<MediaJobRow, Error> {
        let media_id = match self.media_id {
            None => None,
            Some(value) => Some(parse_uuid_v7(&value)?),
        };
        Ok(MediaJobRow {
            id: parse_uuid_v7(&self.id)?,
            owner_id: parse_uuid_v7(&self.owner_id)?,
            media_id,
            kind: self.kind,
            payload: self.payload,
            attempts: self.attempts,
        })
    }
}

impl MediaRepository {
    /// Streamed variant path fingerprint for one blog, scoped to a
    /// transaction. Ordering by media id keeps comparisons stable.
    pub async fn blog_media_fingerprints_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        owner_id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
    ) -> Result<Vec<MediaFingerprint>, Error> {
        use sqlx::Row;
        let fingerprints: Vec<(String, String)> = sqlx::query(
            "SELECT id, status FROM media WHERE blog_id = ? AND owner_id = ? ORDER BY id",
        )
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| internal("media fingerprints"))?
        .into_iter()
        .filter_map(|row| {
            let id = row.try_get::<String, _>(0).ok()?;
            let status = row.try_get::<String, _>(1).ok()?;
            Some((id, status))
        })
        .collect();
        let paths: Vec<(String, String)> = sqlx::query(
            "SELECT v.media_id, v.object_key FROM media_variants v \
             JOIN media m ON m.id = v.media_id \
             WHERE m.blog_id = ? AND m.owner_id = ? ORDER BY v.media_id, v.object_key",
        )
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| internal("media fingerprint paths"))?
        .into_iter()
        .filter_map(|row| {
            let media = row.try_get::<String, _>(0).ok()?;
            let key = row.try_get::<String, _>(1).ok()?;
            Some((media, key))
        })
        .collect();
        let mut result: Vec<MediaFingerprint> = fingerprints
            .into_iter()
            .filter_map(|(id, status)| {
                parse_uuid_v7(&id).ok().map(|uuid| MediaFingerprint {
                    media_id: uuid,
                    status,
                    object_keys: Vec::new(),
                })
            })
            .collect();
        let media_index: std::collections::HashMap<String, usize> = result
            .iter()
            .enumerate()
            .map(|(index, item)| (item.media_id.to_string(), index))
            .collect();
        for (media_id, object_key) in paths {
            if let Some(index) = media_index.get(&media_id) {
                result[*index].object_keys.push(object_key);
            }
        }
        Ok(result)
    }
}
