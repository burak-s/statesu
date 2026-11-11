use sqlx::sqlite::SqlitePool;

use crate::error::{Error, InternalError};
use crate::media::{RawConnectionRow, StorageConnectionRow};

/// SQL-only access to connection and storage-preference tables. Ownership is
/// part of every guarded query, so foreign or unknown entities read as
/// `None` and map to the same 404 as missing ones.
#[derive(Clone)]
pub struct StorageConnectionRepository {
    pool: SqlitePool,
}

impl StorageConnectionRepository {
    pub fn new(pool: SqlitePool) -> StorageConnectionRepository {
        StorageConnectionRepository { pool }
    }

    pub async fn insert(&self, row: &StorageConnectionInsert) -> Result<(), Error> {
        let result = sqlx::query(
            "INSERT INTO storage_connections \
             (id, blog_id, owner_id, name, account_id, bucket, prefix, public_base_url, \
              access_key_id, secret_key, verified_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.id.to_string())
        .bind(row.blog_id.to_string())
        .bind(row.owner_id.to_string())
        .bind(&row.name)
        .bind(&row.account_id)
        .bind(&row.bucket)
        .bind(&row.prefix)
        .bind(&row.public_base_url)
        .bind(&row.access_key_id)
        .bind(&row.sealed_secret)
        .bind(row.verified_at)
        .bind(row.created_at)
        .bind(row.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|err| db_error(err, "connection insert"))?;
        if result.rows_affected() != 1 {
            return Err(internal("connection insert affected no row"));
        }
        Ok(())
    }

    pub async fn count_for_blog(&self, blog_id: &uuid::Uuid) -> Result<i64, Error> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM storage_connections WHERE blog_id = ?")
            .bind(blog_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(|_| internal("connection count"))
    }

    pub async fn count_media_for(&self, connection_id: &uuid::Uuid) -> Result<i64, Error> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM media WHERE connection_id = ?")
            .bind(connection_id.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(|_| internal("connection media count"))
    }

    /// Ownership and blog membership are part of the predicate.
    pub async fn find_for_owner(
        &self,
        owner_id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
        id: &uuid::Uuid,
    ) -> Result<Option<StorageConnectionRow>, Error> {
        sqlx::query_as::<_, RawConnectionRow>(
            "SELECT id, blog_id, owner_id, name, account_id, bucket, prefix, public_base_url, \
             access_key_id, secret_key, verified_at, created_at, updated_at \
             FROM storage_connections WHERE id = ? AND blog_id = ? AND owner_id = ?",
        )
        .bind(id.to_string())
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| internal("connection find"))?
        .map(RawConnectionRow::into_row)
        .transpose()
    }

    pub async fn list_for_blog(
        &self,
        owner_id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
    ) -> Result<Vec<StorageConnectionRow>, Error> {
        let rows = sqlx::query_as::<_, RawConnectionRow>(
            "SELECT id, blog_id, owner_id, name, account_id, bucket, prefix, public_base_url, \
             access_key_id, secret_key, verified_at, created_at, updated_at \
             FROM storage_connections WHERE blog_id = ? AND owner_id = ? \
             ORDER BY created_at DESC, id DESC",
        )
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| internal("connection list"))?;
        rows.into_iter().map(RawConnectionRow::into_row).collect()
    }

    pub async fn rename(
        &self,
        id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
        name: Option<&str>,
        updated_at: i64,
    ) -> Result<(), Error> {
        let result = sqlx::query(
            "UPDATE storage_connections SET name = ?, updated_at = ? \
             WHERE id = ? AND blog_id = ? AND owner_id = ?",
        )
        .bind(name)
        .bind(updated_at)
        .bind(id.to_string())
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|err| db_error(err, "connection rename"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        Ok(())
    }

    /// Rotates credentials after a successful verification.
    pub async fn replace_credentials(
        &self,
        rotation: &CredentialRotation<'_>,
    ) -> Result<(), Error> {
        let result = sqlx::query(
            "UPDATE storage_connections \
             SET access_key_id = ?, secret_key = ?, verified_at = ?, updated_at = ? \
             WHERE id = ? AND blog_id = ? AND owner_id = ?",
        )
        .bind(rotation.access_key_id)
        .bind(rotation.sealed_secret)
        .bind(rotation.verified_at)
        .bind(rotation.updated_at)
        .bind(rotation.id.to_string())
        .bind(rotation.blog_id.to_string())
        .bind(rotation.owner_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|err| db_error(err, "connection credential rotation"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        Ok(())
    }

    /// A connection holding media is never deleted: media objects reference
    /// this row and the objects outlive arbitrary row changes.
    pub async fn delete(
        &self,
        id: &uuid::Uuid,
        blog_id: &uuid::Uuid,
        owner_id: &uuid::Uuid,
    ) -> Result<(), Error> {
        let media_count = self.count_media_for(id).await?;
        if media_count > 0 {
            return Err(Error::ConnectionInUse);
        }
        let result = sqlx::query(
            "DELETE FROM storage_connections WHERE id = ? AND blog_id = ? AND owner_id = ?",
        )
        .bind(id.to_string())
        .bind(blog_id.to_string())
        .bind(owner_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|_| internal("connection delete"))?;
        if result.rows_affected() != 1 {
            return Err(Error::BlogNotFound);
        }
        Ok(())
    }

    /// The blog's active storage preference; `None` when the blog uses the
    /// server-configured bucket or has not chosen anything yet.
    pub async fn blog_connection(
        &self,
        blog_id: &uuid::Uuid,
    ) -> Result<Option<StorageConnectionRow>, Error> {
        let rows = sqlx::query_as::<_, RawConnectionRow>(
            "SELECT c.id, c.blog_id, c.owner_id, c.name, c.account_id, c.bucket, c.prefix, \
             c.public_base_url, c.access_key_id, c.secret_key, c.verified_at, \
             c.created_at, c.updated_at \
             FROM blog_media_storage s JOIN storage_connections c ON c.id = s.connection_id \
             WHERE s.blog_id = ?",
        )
        .bind(blog_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| internal("blog storage preference find"))?;
        rows.map(RawConnectionRow::into_row).transpose()
    }

    /// Replaces the active-storage preference atomically; a connection id
    /// must belong to the same blog, which the causal caller owns.
    pub async fn set_blog_storage(
        &self,
        blog_id: &uuid::Uuid,
        connection_id: &uuid::Uuid,
        updated_at: i64,
    ) -> Result<(), Error> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| internal("connection preference transaction"))?;
        sqlx::query("DELETE FROM blog_media_storage WHERE blog_id = ?")
            .bind(blog_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|_| internal("connection preference delete"))?;
        sqlx::query(
            "INSERT INTO blog_media_storage (blog_id, connection_id, updated_at) \
             VALUES (?, ?, ?)",
        )
        .bind(blog_id.to_string())
        .bind(connection_id.to_string())
        .bind(updated_at)
        .execute(&mut *tx)
        .await
        .map_err(|err| db_error(err, "connection preference insert"))?;
        tx.commit()
            .await
            .map_err(|_| internal("connection preference commit"))
    }

    pub async fn clear_blog_storage(&self, blog_id: &uuid::Uuid) -> Result<(), Error> {
        sqlx::query("DELETE FROM blog_media_storage WHERE blog_id = ?")
            .bind(blog_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|_| internal("connection preference delete"))?;
        Ok(())
    }
}

/// The insert shape: everything `insert` writes, before any ID validation.
pub struct StorageConnectionInsert {
    pub id: uuid::Uuid,
    pub blog_id: uuid::Uuid,
    pub owner_id: uuid::Uuid,
    pub name: Option<String>,
    pub account_id: String,
    pub bucket: String,
    pub prefix: Option<String>,
    pub public_base_url: String,
    pub access_key_id: String,
    pub sealed_secret: Vec<u8>,
    pub verified_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One credential rotation attempt after successful verification.
pub struct CredentialRotation<'a> {
    pub id: &'a uuid::Uuid,
    pub blog_id: &'a uuid::Uuid,
    pub owner_id: &'a uuid::Uuid,
    pub access_key_id: &'a str,
    pub sealed_secret: &'a [u8],
    pub verified_at: i64,
    pub updated_at: i64,
}

pub(crate) fn internal(message: &'static str) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(message)))
}

fn db_error(err: sqlx::Error, context: &'static str) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(format!(
        "{context}: {err}"
    ))))
}
