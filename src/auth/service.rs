use std::sync::Arc;

use crate::auth::pat_repo::{NewPat, PatRecord, PatRepository};
use crate::auth::repo::UserRepository;
use crate::auth::session_repo::{SESSION_ABSOLUTE_TTL, SessionRepository};
use crate::auth::turnstile::TurnstileVerifier;
use crate::auth::verification_repo::{RESEND_COOLDOWN_SECS, VerificationTokenRepository};
use crate::auth::{AcceptedResponse, CredentialsRequest, MeResponse, UserRecord, VerifyRequest};
use crate::clock::Clock;
use crate::crypto::email::EmailCipher;
use crate::crypto::email::{normalize_email, validate_email};
use crate::crypto::password::PasswordHasher;
use crate::crypto::token;
use crate::error::Error;
use crate::mail::Mailer;

/// Account rules: registration+verification, credential checks, browser
/// sessions and personal access tokens. Only opaque token hashes are stored;
/// secrets exist exactly once, at the moment they are handed out.
#[derive(Clone)]
pub struct AuthService {
    users: UserRepository,
    sessions: SessionRepository,
    verifications: VerificationTokenRepository,
    pats: PatRepository,
    hasher: PasswordHasher,
    cipher: EmailCipher,
    mailer: Arc<dyn Mailer>,
    turnstile: Option<Arc<dyn TurnstileVerifier>>,
    clock: Arc<dyn Clock>,
    public_url: crate::config::PublicUrl,
}

/// Personal access token per-account quota.
pub const PAT_MAX_ACTIVE: i64 = 20;
pub const PAT_MAX_NAME_LEN: usize = 64;

pub const SCOPE_READ: &str = "content:read";
pub const SCOPE_WRITE: &str = "content:write";

/// What a successful login hands back: identity plus the cookie secrets.
pub struct LoginOutcome {
    pub me: MeResponse,
    pub session_token: String,
    pub csrf_token: String,
    /// Cookie lifetime mirror of the server-side absolute expiry.
    pub max_age_secs: i64,
}

/// A freshly minted personal access token: metadata plus the one-time secret.
pub struct PatOutcome {
    pub record: PatRecord,
    /// Secret shown exactly once, prefixed `su_pat_`.
    pub secret: String,
}

impl AuthService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        users: UserRepository,
        sessions: SessionRepository,
        verifications: VerificationTokenRepository,
        pats: PatRepository,
        hasher: PasswordHasher,
        cipher: EmailCipher,
        mailer: Arc<dyn Mailer>,
        turnstile: Option<Arc<dyn TurnstileVerifier>>,
        clock: Arc<dyn Clock>,
        public_url: crate::config::PublicUrl,
    ) -> AuthService {
        AuthService {
            users,
            sessions,
            verifications,
            pats,
            hasher,
            cipher,
            mailer,
            turnstile,
            clock,
            public_url,
        }
    }

    /// The human gate for register and login: with no secret configured the
    /// check vanishes; otherwise a missing or empty token counts as a
    /// rejection, identically to an unsolved challenge.
    async fn verify_turnstile(&self, token: Option<&str>) -> Result<(), Error> {
        match self.turnstile.as_ref() {
            None => Ok(()),
            Some(verifier) => {
                let token = token.filter(|t| !t.trim().is_empty());
                match token {
                    Some(token) => verifier.verify(token, None).await,
                    None => Err(Error::TurnstileFailed),
                }
            }
        }
    }

    /// Creates an unverified account and starts address confirmation; no
    /// credential of any kind is issued before the address is verified.
    pub async fn register(&self, request: &CredentialsRequest) -> Result<AcceptedResponse, Error> {
        self.verify_turnstile(request.turnstile.as_deref()).await?;
        let email = normalize_email(&request.email);
        validate_email(&email)?;
        crate::crypto::password::check_password(&request.password)?;

        let user_id = uuid::Uuid::now_v7();
        let ciphertext = self.cipher.encrypt(&email, &user_id)?;
        let email_hash = self.cipher.email_hash(&email);

        // Hash before uniqueness checks so existing accounts and new ones cost
        // the same, keeping register timing membership-neutral.
        let password_hash = self.hasher.hash(&request.password).await?;
        self.users
            .create(&user_id, &ciphertext, &email_hash, &password_hash)
            .await?;
        self.send_verification_mail(&email, &user_id).await;
        Ok(AcceptedResponse::VERIFICATION_REQUIRED)
    }

    /// Credential check plus session minting. Unverified accounts are
    /// rejected after the same generic failure pattern as wrong passwords.
    pub async fn login(&self, request: &CredentialsRequest) -> Result<LoginOutcome, Error> {
        self.verify_turnstile(request.turnstile.as_deref()).await?;
        let email = normalize_email(&request.email);
        if email.is_empty() || request.password.is_empty() {
            return Err(Error::InvalidCredentials);
        }
        let email_hash = self.cipher.email_hash(&email);
        let user = match self.users.find_by_email_hash(&email_hash).await? {
            Some(user) => user,
            None => {
                self.hasher.verify_dummy(&request.password).await?;
                return Err(Error::InvalidCredentials);
            }
        };
        if !self
            .hasher
            .verify(&request.password, &user.password_hash)
            .await?
        {
            return Err(Error::InvalidCredentials);
        }
        if !user.is_verified() {
            return Err(Error::EmailNotVerified);
        }

        let session_token = token::generate(token::SESSION_PREFIX)?;
        let csrf_token = token::generate(token::CSRF_PREFIX)?;
        let session_hash = token::hash(&session_token);
        let csrf_hash = token::hash(&csrf_token).to_vec();
        let now = self.clock.now();
        self.sessions
            .create(&session_hash, &csrf_hash, &user.id, now)
            .await?;
        let email = self.cipher.decrypt(&user.email_ciphertext, &user.id)?;
        Ok(LoginOutcome {
            me: MeResponse { id: user.id, email },
            session_token,
            csrf_token,
            max_age_secs: SESSION_ABSOLUTE_TTL,
        })
    }

    pub async fn me(&self, user: &UserRecord) -> Result<MeResponse, Error> {
        let email = self.cipher.decrypt(&user.email_ciphertext, &user.id)?;
        Ok(MeResponse { id: user.id, email })
    }

    /// Session-cookie authentication: strict hash lookup plus expiry checks,
    /// so revoked and expired sessions fail closed as `invalid token`.
    pub async fn authenticate_session(
        &self,
        session_token: &str,
    ) -> Result<(UserRecord, Vec<u8>), Error> {
        let hash = token::hash(session_token);
        let now = self.clock.now();
        let session = self
            .sessions
            .find_active(&hash, now)
            .await?
            .ok_or(Error::InvalidToken)?;
        let user = self
            .users
            .find_by_id(&session.user_id)
            .await?
            .ok_or(Error::InvalidToken)?;
        self.sessions.touch(&hash, now).await?;
        Ok((user, session.csrf_hash))
    }

    /// Personal access token authentication for the management API.
    pub async fn authenticate_pat(
        &self,
        pat_token: &str,
    ) -> Result<(UserRecord, PatScopes), Error> {
        let hash = token::hash(pat_token);
        let now = self.clock.now();
        let (user_id, scopes) = self
            .pats
            .find_active(&hash, now)
            .await?
            .ok_or(Error::InvalidToken)?;
        let user = self
            .users
            .find_by_id(&user_id)
            .await?
            .ok_or(Error::InvalidToken)?;
        self.pats.update_last_used(&hash, now).await?;
        Ok((user, PatScopes::parse(&scopes)?))
    }

    /// `POST /auth/logout`: revokes the presented session.
    pub async fn logout(&self, session_token: &str) -> Result<bool, Error> {
        self.sessions.delete(&token::hash(session_token)).await
    }

    /// "Sign out everywhere": revokes every session of the account.
    pub async fn logout_all(&self, user_id: &uuid::Uuid) -> Result<u64, Error> {
        self.sessions.delete_all_for_user(user_id).await
    }

    /// Consumes a verification token and confirms the address. Replay,
    /// expiry and racing consumers each fail closed.
    pub async fn verify_email(&self, request: &VerifyRequest) -> Result<(), Error> {
        if !token::well_formed(&request.token, token::VERIFICATION_PREFIX) {
            return Err(Error::InvalidToken);
        }
        let hash = token::hash(&request.token);
        let now = self.clock.now();
        let row = self
            .verifications
            .find(&hash)
            .await?
            .ok_or(Error::InvalidToken)?;
        if row.consumed_at.is_some() {
            return Err(Error::InvalidToken);
        }
        if row.expires_at <= now {
            return Err(Error::TokenExpired);
        }
        // Two concurrent consumers race on this conditional update; the
        // loser's no-op already implies the account got verified.
        let consumed = self.verifications.consume(&hash, now).await?;
        if consumed {
            self.users.mark_verified(&row.user_id, now).await?;
        }
        Ok(())
    }

    /// Generic resend: identical response for unknown, invalid, verified and
    /// throttled addresses; sends a new mail only when useful and allowed.
    pub async fn resend_verification(&self, email: &str) -> Result<AcceptedResponse, Error> {
        let email = normalize_email(email);
        if validate_email(&email).is_ok() {
            let email_hash = self.cipher.email_hash(&email);
            if let Some(user) = self.users.find_by_email_hash(&email_hash).await?
                && !user.is_verified()
                && !self.cooling_down(&user.id).await
            {
                self.send_verification_mail(&email, &user.id).await;
            }
        }
        Ok(AcceptedResponse::VERIFICATION_REQUIRED)
    }

    /// True while the account's newest verification mail is still young.
    async fn cooling_down(&self, user_id: &uuid::Uuid) -> bool {
        let latest = self
            .verifications
            .latest_for_user(user_id)
            .await
            .ok()
            .flatten();
        latest.is_some_and(|row| {
            let now = self.clock.now();
            now < row.created_at.saturating_add(RESEND_COOLDOWN_SECS)
        })
    }

    /// Issues and mails a fresh verification token, replacing any earlier one.
    /// Delivery failures are logged; the account stays recoverable via resend.
    async fn send_verification_mail(&self, email: &str, user_id: &uuid::Uuid) {
        let issued = async {
            let verification_token = token::generate(token::VERIFICATION_PREFIX)?;
            self.verifications
                .issue(&token::hash(&verification_token), user_id, self.clock.now())
                .await?;
            Ok::<_, Error>(verification_link(&self.public_url, &verification_token))
        }
        .await;
        match issued {
            Ok(link) => {
                if let Err(err) = self.mailer.send_verification(email, &link).await {
                    tracing::error!(error = %err, "verification mail delivery failed");
                }
            }
            Err(err) => tracing::error!(error = %err, "verification token issue failed"),
        }
    }

    pub async fn create_pat(
        &self,
        user_id: &uuid::Uuid,
        name: &str,
        scopes: PatScopes,
        expires_at: Option<i64>,
    ) -> Result<PatOutcome, Error> {
        let name = name.trim();
        if name.is_empty() || name.len() > PAT_MAX_NAME_LEN {
            return Err(Error::InvalidInput);
        }
        if expires_at.is_some_and(|expiry| expiry <= self.clock.now()) {
            return Err(Error::InvalidInput);
        }
        let limit_reached =
            self.pats.count_active(user_id, self.clock.now()).await? >= PAT_MAX_ACTIVE;
        if limit_reached {
            return Err(Error::Forbidden);
        }

        let now = self.clock.now();
        let id = uuid::Uuid::now_v7();
        let secret = token::generate(token::PAT_PREFIX)?;
        let token_hash = token::hash(&secret);
        self.pats
            .create(
                NewPat {
                    id,
                    user_id: *user_id,
                    name: name.to_owned(),
                    scopes: scopes.as_stored(),
                    created_at: now,
                    expires_at,
                },
                &token_hash,
            )
            .await?;
        let record = self
            .pats
            .find_for_user(&id, user_id)
            .await?
            .expect("token row was just inserted");
        Ok(PatOutcome { record, secret })
    }

    pub async fn list_pats(&self, user_id: &uuid::Uuid) -> Result<Vec<PatRecord>, Error> {
        self.pats.list_for_user(user_id).await
    }

    pub async fn delete_pat(&self, pat_id: &uuid::Uuid, user_id: &uuid::Uuid) -> Result<(), Error> {
        if !self.pats.delete(pat_id, user_id).await? {
            return Err(Error::PatNotFound);
        }
        Ok(())
    }
}

/// Absolute link for a verification token, built only from the configured
/// public origin so host headers can never poison it.
fn verification_link(public_url: &crate::config::PublicUrl, token: &str) -> String {
    public_url.link(&format!("/verify?token={token}"))
}

/// Validated PAT scopes: a subset of `content:read` / `content:write`; the
/// write scope implies read on the enforcement side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatScopes {
    read: bool,
    write: bool,
}

impl PatScopes {
    /// Default new-token scope set: read-only content access.
    pub fn read_only() -> PatScopes {
        PatScopes {
            read: true,
            write: false,
        }
    }

    /// Read + write scope set.
    pub fn read_write() -> PatScopes {
        PatScopes {
            read: true,
            write: true,
        }
    }

    /// Parses a stored (or wire-supplied) scope list; unknown names are a
    /// caller bug or bad input and both are rejected.
    pub fn parse(stored: &str) -> Result<PatScopes, Error> {
        let mut read = false;
        let mut write = false;
        for scope in stored.split_whitespace() {
            match scope {
                SCOPE_READ => read = true,
                SCOPE_WRITE => write = true,
                _ => {
                    return Err(Error::Internal(crate::error::InternalError::new(
                        std::io::Error::other(format!("unknown stored scope: {scope}")),
                    )));
                }
            }
        }
        Ok(PatScopes { read, write })
    }

    pub fn can_read(&self) -> bool {
        self.read || self.write
    }

    pub fn can_write(&self) -> bool {
        self.write
    }

    fn as_stored(&self) -> String {
        match (self.read, self.write) {
            (true, true) => format!("{SCOPE_READ} {SCOPE_WRITE}"),
            (true, false) => SCOPE_READ.to_owned(),
            (false, true) => SCOPE_WRITE.to_owned(),
            (false, false) => String::new(),
        }
    }
}
