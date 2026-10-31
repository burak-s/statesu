use sqlx::sqlite::SqlitePool;

use crate::error::Error;

/// SQL-only access to the `sessions` table: durable, revocable browser
/// sessions keyed by token hash. Every method takes pre-hashed tokens.
#[derive(Clone)]
pub struct SessionRepository {
    pool: SqlitePool,
}

/// One durable session row.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub user_id: uuid::Uuid,
    pub csrf_hash: Vec<u8>,
    pub last_seen_at: i64,
    pub expires_at: i64,
}

const SESSION_IDLE_TTL_SECS: i64 = 24 * 60 * 60;
const SESSION_ABSOLUTE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

pub const SESSION_IDLE_TTL: i64 = SESSION_IDLE_TTL_SECS;
pub const SESSION_ABSOLUTE_TTL: i64 = SESSION_ABSOLUTE_TTL_SECS;

impl SessionRepository {
    pub fn new(pool: SqlitePool) -> SessionRepository {
        SessionRepository { pool }
    }

    /// Inserts a freshly minted session.
    pub async fn create(
        &self,
        token_hash: &[u8; 32],
        csrf_hash: &[u8],
        user_id: &uuid::Uuid,
        now_unix: i64,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO sessions (token_hash, user_id, csrf_hash, created_at, last_seen_at, expires_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(token_hash.as_slice())
        .bind(user_id.to_string())
        .bind(csrf_hash)
        .bind(now_unix)
        .bind(now_unix)
        .bind(now_unix.checked_add(SESSION_ABSOLUTE_TTL_SECS).expect("session ttl"))
        .execute(&self.pool)
        .await
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(())
    }

    /// Loads an unexpired session by token hash. The idle window is enforced
    /// against `last_seen_at`; expired rows are swept opportunistically.
    pub async fn find_active(
        &self,
        token_hash: &[u8; 32],
        now_unix: i64,
    ) -> Result<Option<SessionRow>, Error> {
        let row = sqlx::query_as::<_, SessionRecordRow>(
            "SELECT user_id, csrf_hash, last_seen_at, expires_at \
             FROM sessions WHERE token_hash = ?",
        )
        .bind(token_hash.as_slice())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        let Some(row) = row else { return Ok(None) };
        let idle_deadline = row.last_seen_at.saturating_add(SESSION_IDLE_TTL_SECS);
        if row.expires_at <= now_unix || idle_deadline <= now_unix {
            self.delete(token_hash).await?;
            return Ok(None);
        }
        Ok(Some(SessionRow {
            user_id: row
                .user_id
                .parse()
                .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?,
            csrf_hash: row.csrf_hash,
            last_seen_at: row.last_seen_at,
            expires_at: row.expires_at,
        }))
    }

    /// Sliding-window activity marker; one small write per request.
    pub async fn touch(&self, token_hash: &[u8; 32], now_unix: i64) -> Result<(), Error> {
        sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
            .bind(now_unix)
            .bind(token_hash.as_slice())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(())
    }

    /// Deletes one session; `false` when the row was already gone.
    pub async fn delete(&self, token_hash: &[u8; 32]) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash.as_slice())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(result.rows_affected() > 0)
    }

    /// "Sign out everywhere": drops every session of the user.
    pub async fn delete_all_for_user(&self, user_id: &uuid::Uuid) -> Result<u64, Error> {
        let result = sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(user_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| Error::Internal(crate::error::InternalError::new(e)))?;
        Ok(result.rows_affected())
    }
}

#[derive(sqlx::FromRow)]
struct SessionRecordRow {
    user_id: String,
    csrf_hash: Vec<u8>,
    last_seen_at: i64,
    expires_at: i64,
}
