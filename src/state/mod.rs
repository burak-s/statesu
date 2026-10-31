pub mod repo;
pub mod service;

use uuid::Uuid;

use crate::error::Error;

/// Storage shape for the `states` table.
#[derive(Debug, Clone)]
pub struct StateRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub text: String,
    pub created_at: i64,
    pub expires_at: i64,
}

/// `sqlx::FromRow` projection of the `states` table before ID validation.
#[derive(Debug, sqlx::FromRow)]
pub struct RawStateRow {
    id: String,
    user_id: String,
    text: String,
    created_at: i64,
    expires_at: i64,
}

impl RawStateRow {
    pub fn into_state_row(self) -> Result<StateRow, Error> {
        Ok(StateRow {
            id: parse_state_id(&self.id)?,
            user_id: parse_state_id(&self.user_id)?,
            text: self.text,
            created_at: self.created_at,
            expires_at: self.expires_at,
        })
    }
}

/// Path IDs are validated as UUIDv7 before they reach SQL.
pub fn parse_state_id(text: &str) -> Result<Uuid, Error> {
    let parsed = text.parse::<Uuid>().map_err(|_| Error::InvalidInput)?;
    if parsed.get_version_num() == 7 {
        Ok(parsed)
    } else {
        Err(Error::InvalidInput)
    }
}
