-- Auth hardening: email verification, opaque session cookies, personal
-- access tokens. Secrets are never stored in plaintext: every token row
-- keeps only the SHA-256 hash of the token string.
ALTER TABLE users ADD COLUMN email_verified_at INTEGER;

-- Browser dashboard sessions. `csrf_hash` binds the readable CSRF cookie to
-- the server-side session, so the pair must both leak to be abused.
CREATE TABLE sessions (
    token_hash   BLOB PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_hash    BLOB NOT NULL,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL
);

CREATE INDEX idx_sessions_user ON sessions (user_id, expires_at);

-- Single-use verification tokens; a resend replaces any earlier token.
CREATE TABLE email_verification_tokens (
    token_hash  BLOB PRIMARY KEY,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL,
    consumed_at INTEGER
);

CREATE INDEX idx_verification_user ON email_verification_tokens (user_id, created_at DESC);

-- Personal access tokens for API/CLI clients: named, scoped, expiring,
-- revocable. Scopes are space-separated (`content:read`, `content:write`).
CREATE TABLE pats (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   BLOB NOT NULL UNIQUE,
    scopes       TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    last_used_at INTEGER
);

CREATE INDEX idx_pats_user ON pats (user_id, created_at DESC);
