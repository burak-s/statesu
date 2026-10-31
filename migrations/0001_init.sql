-- Statesu fresh schema. User and state IDs are canonical UUIDv7 TEXT.
-- Timestamps are INTEGER Unix seconds (UTC), not driver-dependent datetimes.
CREATE TABLE users (
    id               TEXT PRIMARY KEY,
    email_ciphertext BLOB NOT NULL,
    email_hash       BLOB NOT NULL UNIQUE,
    password_hash    TEXT NOT NULL
);

CREATE TABLE states (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

-- Deterministic per-user listing order (newest first).
CREATE INDEX idx_states_user_created
    ON states (user_id, created_at DESC, id DESC);

-- Deterministic global latest order (newest first).
CREATE INDEX idx_states_created
    ON states (created_at DESC, id DESC);
