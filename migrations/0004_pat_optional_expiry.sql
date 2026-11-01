-- A null expiry represents a token that remains valid until revoked.
CREATE TABLE pats_new (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   BLOB NOT NULL UNIQUE,
    scopes       TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER,
    last_used_at INTEGER
);

INSERT INTO pats_new SELECT id, user_id, name, token_hash, scopes, created_at, expires_at, last_used_at FROM pats;
DROP TABLE pats;
ALTER TABLE pats_new RENAME TO pats;
CREATE INDEX idx_pats_user ON pats (user_id, created_at DESC);
