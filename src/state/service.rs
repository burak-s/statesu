use std::sync::Arc;

use crate::auth::UserRecord;
use crate::auth::repo::UserRepository;
use crate::clock::Clock;
use crate::crypto::email::EmailCipher;
use crate::error::Error;
use crate::state::repo::StateRepository;
use crate::state::{StateRow, parse_state_id};

/// 30 days in seconds; expiration must stay within this horizon.
pub const MAX_EXPIRY_SECS: i64 = 30 * 24 * 60 * 60;

/// Upper byte length for a state's text after trimming.
pub const MAX_TEXT_BYTES: usize = 4096;

/// Pagination defaults from the public contract.
pub const DEFAULT_PAGE: u32 = 1;
pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 100;

/// State rules: text/expiry validation, ordering, pagination and ownership.
/// Repos and crypto components are injected; no globals.
#[derive(Clone)]
pub struct StateService {
    states: StateRepository,
    users: UserRepository,
    cipher: EmailCipher,
    clock: Arc<dyn Clock>,
}

/// Wire shape for create/list items.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StateResponse {
    pub state_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub text: String,
    pub created_at: i64,
    pub expires_at: i64,
}

/// Wire shape for the global/per-user latest read, including owner email.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatestStateResponse {
    pub state_id: uuid::Uuid,
    pub user_id: uuid::Uuid,
    pub email: String,
    pub text: String,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ListResponse {
    pub items: Vec<StateResponse>,
    pub page: u32,
    pub size: u32,
    pub total: i64,
}

/// Strict JSON request body for creating a state.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateStateRequest {
    pub text: String,
    pub expires_at: i64,
}

impl StateService {
    pub fn new(
        states: StateRepository,
        users: UserRepository,
        cipher: EmailCipher,
        clock: Arc<dyn Clock>,
    ) -> StateService {
        StateService {
            states,
            users,
            cipher,
            clock,
        }
    }

    pub async fn create(
        &self,
        user_id: uuid::Uuid,
        request: &CreateStateRequest,
    ) -> Result<StateResponse, Error> {
        let now = self.clock.now();
        let text = request.text.trim();
        if text.is_empty() || text.len() > MAX_TEXT_BYTES {
            return Err(Error::InvalidInput);
        }
        let Some(max_expiry) = now.checked_add(MAX_EXPIRY_SECS) else {
            return Err(Error::InvalidInput);
        };
        if request.expires_at <= now || request.expires_at > max_expiry {
            return Err(Error::InvalidInput);
        }
        let state_id = uuid::Uuid::now_v7();
        self.states
            .insert(&state_id, &user_id, text, now, request.expires_at)
            .await?;
        Ok(StateResponse {
            state_id,
            user_id,
            text: text.to_owned(),
            created_at: now,
            expires_at: request.expires_at,
        })
    }

    /// Public listing by owner email; unknown emails read as empty lists so
    /// callers cannot use the endpoint for account enumeration.
    pub async fn list(
        &self,
        email_param: Option<String>,
        page_param: Option<&str>,
        size_param: Option<&str>,
    ) -> Result<ListResponse, Error> {
        let Some(email) = email_param
            .map(|e| e.trim().to_owned())
            .filter(|e| !e.is_empty())
        else {
            return Err(Error::MissingEmail);
        };
        let page = parse_page_number(page_param)?;
        let size = parse_page_size(size_param)?;
        let user = self
            .users
            .find_by_email_hash(&self.cipher.email_hash(&email))
            .await?;
        let Some(user) = user else {
            return Ok(ListResponse {
                items: Vec::new(),
                page,
                size,
                total: 0,
            });
        };
        // Page and size are u32 with size <= 100, so the offset stays far
        // inside the i64 range SQLite expects.
        let offset = u64::from(page.saturating_sub(1)) * u64::from(size);
        let rows = self
            .states
            .page_for_user(
                &user.id,
                i64::from(size),
                i64::try_from(offset).map_err(|_| Error::InvalidInput)?,
            )
            .await?;
        let total = self.states.count_for_user(&user.id).await?;
        Ok(ListResponse {
            items: rows.into_iter().map(make_state_response).collect(),
            page,
            size,
            total,
        })
    }

    /// Public read of the newest state, optionally scoped by email. Expired
    /// states stay returned; expiration is metadata, not deletion.
    pub async fn latest(&self, email_param: Option<String>) -> Result<LatestStateResponse, Error> {
        let scoped_user: Option<UserRecord> = match email_param {
            Some(email) => {
                let email = email.trim();
                if email.is_empty() {
                    None
                } else {
                    Some(
                        self.users
                            .find_by_email_hash(&self.cipher.email_hash(email))
                            .await?
                            .ok_or(Error::StateNotFound)?,
                    )
                }
            }
            None => None,
        };
        let row = match scoped_user.as_ref() {
            Some(user) => self.states.latest_for_user(&user.id).await?,
            None => self.states.latest_global().await?,
        };
        let Some(row) = row else {
            return Err(Error::StateNotFound);
        };
        let email = match scoped_user {
            Some(user) => self.cipher.decrypt(&user.email_ciphertext, &user.id)?,
            None => {
                let owner = self
                    .users
                    .find_by_id(&row.user_id)
                    .await?
                    .ok_or(Error::StateNotFound)?;
                self.cipher.decrypt(&owner.email_ciphertext, &owner.id)?
            }
        };
        let state = make_state_response(row);
        Ok(LatestStateResponse {
            state_id: state.state_id,
            user_id: state.user_id,
            email,
            text: state.text,
            created_at: state.created_at,
            expires_at: state.expires_at,
        })
    }

    pub async fn delete(&self, user_id: uuid::Uuid, state_id_param: &str) -> Result<(), Error> {
        let state_id = parse_state_id(state_id_param)?;
        let deleted = self.states.delete(&state_id, &user_id).await?;
        if deleted {
            Ok(())
        } else {
            Err(Error::StateNotFound)
        }
    }
}

fn make_state_response(row: StateRow) -> StateResponse {
    StateResponse {
        state_id: row.id,
        user_id: row.user_id,
        text: row.text,
        created_at: row.created_at,
        expires_at: row.expires_at,
    }
}

/// Pagination is opt-in: absent is the default, rejected when malformed,
/// non-positive or overflowing.
fn parse_page_number(param: Option<&str>) -> Result<u32, Error> {
    match param {
        None => Ok(DEFAULT_PAGE),
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| Error::InvalidInput)
            .and_then(|p| {
                if p >= 1 {
                    Ok(p)
                } else {
                    Err(Error::InvalidInput)
                }
            }),
    }
}

fn parse_page_size(param: Option<&str>) -> Result<u32, Error> {
    match param {
        None => Ok(DEFAULT_PAGE_SIZE),
        Some(value) => value
            .parse::<u32>()
            .map_err(|_| Error::InvalidInput)
            .and_then(|size| {
                if size >= 1 {
                    Ok(size.min(MAX_PAGE_SIZE))
                } else {
                    Err(Error::InvalidInput)
                }
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;
    use std::sync::Arc;

    #[test]
    fn pagination_defaults_and_capping() {
        assert_eq!(parse_page_number(None).unwrap(), 1);
        assert!(matches!(
            parse_page_number(Some("abc")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_number(Some("0")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_number(Some("-2")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_number(Some("99999999999999999999999")),
            Err(Error::InvalidInput)
        ));
        assert_eq!(parse_page_number(Some("3")).unwrap(), 3);

        assert_eq!(parse_page_size(None).unwrap(), 20);
        assert!(matches!(
            parse_page_size(Some("abc")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_size(Some("0")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_size(Some("-2")),
            Err(Error::InvalidInput)
        ));
        assert!(matches!(
            parse_page_size(Some("99999999999999999999")),
            Err(Error::InvalidInput)
        ));
        assert_eq!(parse_page_size(Some("500")).unwrap(), 100);
        assert_eq!(parse_page_size(Some("77")).unwrap(), 77);
    }

    #[test]
    fn max_expiry_is_thirty_days() {
        assert_eq!(MAX_EXPIRY_SECS, 30 * 24 * 60 * 60);
    }

    #[test]
    fn test_clock_is_a_clock() -> Result<(), Error> {
        let clock: Arc<dyn Clock> = Arc::new(TestClock::at_start(1_700_000_000));
        assert_eq!(clock.now(), 1_700_000_000);
        Ok(())
    }
}
