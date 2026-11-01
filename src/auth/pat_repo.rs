use sqlx::sqlite::SqlitePool;

use crate::error::Error;

/// SQL-only access to the `pats` table: named, scoped, expiring personal
/// access tokens keyed by token hash. Secrets exist only in the create
/// response; every read returns metadata.
#[derive(Clone)]
pub struct PatRepository {
    pool: SqlitePool,
}

const PAT_METADATA_COLUMNS: &str = "id, name, scopes, created_at, expires_at, last_used_at";

/// Everything a new PAT row needs except its (already hashed) secret.
pub struct NewPat {
    pub id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub name: String,
    pub scopes: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

/// One non-secret PAT row, shaped for both the repository and the API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PatRecord {
    pub id: uuid::Uuid,
    pub name: String,
    pub scopes: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
}

impl PatRepository {
    pub fn new(pool: SqlitePool) -> PatRepository {
        PatRepository { pool }
    }

    pub async fn create(&self, pat: NewPat, token_hash: &[u8; 32]) -> Result<(), Error> {
        let NewPat {
            id,
            user_id,
            name,
            scopes,
            created_at,
            expires_at,
        } = pat;
        sqlx::query(
            "INSERT INTO pats (id, user_id, name, token_hash, scopes, created_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.to_string())
        .bind(user_id.to_string())
        .bind(name)
        .bind(token_hash.as_slice())
        .bind(scopes)
        .bind(created_at)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(())
    }

    /// Resolves an unexpired token to its owner and scopes.
    pub async fn find_active(
        &self,
        token_hash: &[u8; 32],
        now_unix: i64,
    ) -> Result<Option<(uuid::Uuid, String)>, Error> {
        let row = sqlx::query_as::<_, TokenOwnerRow>(
            "SELECT user_id, scopes FROM pats WHERE token_hash = ? AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(token_hash.as_slice())
        .bind(now_unix)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(row.map(|row| {
            (
                uuid::Uuid::parse_str(&row.user_id).expect("schema stores canonical UUID text"),
                row.scopes,
            )
        }))
    }

    pub async fn update_last_used(
        &self,
        token_hash: &[u8; 32],
        now_unix: i64,
    ) -> Result<(), Error> {
        sqlx::query("UPDATE pats SET last_used_at = ? WHERE token_hash = ?")
            .bind(now_unix)
            .bind(token_hash.as_slice())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(())
    }

    pub async fn list_for_user(&self, user_id: &uuid::Uuid) -> Result<Vec<PatRecord>, Error> {
        sqlx::query_as::<_, PatRow>(
            &format!("SELECT {PAT_METADATA_COLUMNS} FROM pats WHERE user_id = ? ORDER BY created_at DESC, id DESC"),
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map(|rows| rows.into_iter().map(Into::into).collect())
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))
    }

    pub async fn find_for_user(
        &self,
        pat_id: &uuid::Uuid,
        user_id: &uuid::Uuid,
    ) -> Result<Option<PatRecord>, Error> {
        sqlx::query_as::<_, PatRow>(&format!(
            "SELECT {PAT_METADATA_COLUMNS} FROM pats WHERE id = ? AND user_id = ?"
        ))
        .bind(pat_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Into::into))
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))
    }

    pub async fn delete(&self, pat_id: &uuid::Uuid, user_id: &uuid::Uuid) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM pats WHERE id = ? AND user_id = ?")
            .bind(pat_id.to_string())
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(result.rows_affected() > 0)
    }

    /// ActivePAT cap per account; checked before minting a new token.
    pub async fn count_active(&self, user_id: &uuid::Uuid, now_unix: i64) -> Result<i64, Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM pats WHERE user_id = ? AND (expires_at IS NULL OR expires_at > ?)")
            .bind(user_id.to_string())
            .bind(now_unix)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))
    }
}

#[derive(sqlx::FromRow)]
struct TokenOwnerRow {
    user_id: String,
    scopes: String,
}

#[derive(sqlx::FromRow)]
struct PatRow {
    id: String,
    name: String,
    scopes: String,
    created_at: i64,
    expires_at: Option<i64>,
    last_used_at: Option<i64>,
}

impl From<PatRow> for PatRecord {
    fn from(row: PatRow) -> PatRecord {
        PatRecord {
            id: uuid::Uuid::parse_str(&row.id).expect("schema stores canonical UUID text"),
            name: row.name,
            scopes: row.scopes,
            created_at: row.created_at,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
        }
    }
}
