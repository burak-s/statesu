-- Media storage: user-connectable R2 buckets ("storage connections"), the
-- per-blog active connection preference, and the media rows the system
-- stores. IDs are UUIDv7 TEXT; timestamps are INTEGER Unix seconds (UTC).
--
-- Media objects are public by design: access control happens once per
-- object key, not per request. Media rows permanently reference the
-- connection they were stored with; switching a blog to another connection
-- never re-parents old media, and deleting a connection that still holds
-- media is refused.

CREATE TABLE storage_connections (
    id              TEXT PRIMARY KEY,
    blog_id         TEXT NOT NULL REFERENCES blogs(id) ON DELETE CASCADE,
    owner_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT,
    account_id      TEXT NOT NULL,
    bucket          TEXT NOT NULL,
    prefix          TEXT,
    public_base_url TEXT NOT NULL,
    access_key_id   TEXT NOT NULL,
    /* AES-256-GCM blob: version byte + nonce + ciphertext, AAD-bound to id */
    secret_key      BLOB NOT NULL,
    verified_at     INTEGER NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE INDEX idx_storage_connections_blog
    ON storage_connections (blog_id, created_at DESC, id DESC);

-- Row presence means "use the referenced custom connection for new media";
-- no row means Statesu's server-configured bucket.
CREATE TABLE blog_media_storage (
    blog_id       TEXT PRIMARY KEY REFERENCES blogs(id) ON DELETE CASCADE,
    connection_id TEXT NOT NULL REFERENCES storage_connections(id) ON DELETE CASCADE,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE media (
    id             TEXT PRIMARY KEY,
    blog_id        TEXT NOT NULL REFERENCES blogs(id) ON DELETE CASCADE,
    owner_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- NULL means the server's own bucket (STATESU_R2_* config);
    -- a value references the committed-on row, which blocks connection deletion.
    connection_id  TEXT REFERENCES storage_connections(id) ON DELETE RESTRICT,
    -- Publishing base snapshotted at commit time: the server bucket's URL
    -- is resolved from this, so config changes never rewrite history and
    -- object cleanup keeps its original target.
    public_base_url TEXT NOT NULL,
    -- Location identity for server-bucket rows (JSON: account_id, bucket,
    -- prefix, public_base_url — secrets excluded). Pins the exact bucket
    -- the objects live in without copying server secrets into the DB;
    -- cleanup re-resolves credentials from the current config when the
    -- location identity still exists. NULL for custom-connection rows,
    -- whose material lives in the (deletion-blocked) connection row.
    storage_location BLOB,
    -- uploading: reserve held, objects being written; ready: served;
    -- deleting: objects being removed by durable jobs; error: failed
    -- terminal state, cleaned up
    status         TEXT NOT NULL DEFAULT 'uploading'
                   CHECK (status IN ('uploading', 'ready', 'deleting', 'error')),
    mime           TEXT NOT NULL,
    width          INTEGER,
    height         INTEGER,
    -- Actual bytes stored across all variants
    stored_bytes   INTEGER NOT NULL DEFAULT 0,
    -- Quota bytes held while uploading; folded into stored_bytes on success
    reserved_bytes INTEGER NOT NULL DEFAULT 0,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE INDEX idx_media_owner ON media (owner_id);
CREATE INDEX idx_media_connection ON media (connection_id);

-- Object location and byte accounting per generated variant;
-- label 'source' is the cleaned source image.
CREATE TABLE media_variants (
    media_id   TEXT NOT NULL REFERENCES media(id) ON DELETE CASCADE,
    label      TEXT NOT NULL,
    object_key TEXT NOT NULL,
    mime       TEXT NOT NULL,
    width      INTEGER,
    height     INTEGER,
    bytes      INTEGER NOT NULL,
    PRIMARY KEY (media_id, label)
);

-- Durable work for processing, deletion and cleanup retries, kept alive by
-- `owner_id` independent of blog/media deletion so quota accounting and
-- remote-object cleanup never depend on one process or HTTP request.
-- `payload` snapshots the object keys plus the connection stamp needed to
-- reach them; `last_error` records why a retry is due.
CREATE TABLE media_jobs (
    id          TEXT PRIMARY KEY,
    owner_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_id    TEXT,
    blog_id     TEXT,
    kind        TEXT NOT NULL CHECK (kind IN ('process', 'delete', 'cleanup')),
    payload     TEXT NOT NULL,
    attempts    INTEGER NOT NULL DEFAULT 0,
    next_run_at INTEGER NOT NULL,
    last_error  TEXT,
    created_at  INTEGER NOT NULL
);

CREATE INDEX idx_media_jobs_due ON media_jobs (next_run_at, id DESC);
