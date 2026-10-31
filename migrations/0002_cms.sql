-- CMS schema: blogs, per-blog browser origins and posts. IDs are canonical
-- UUIDv7 TEXT; timestamps are INTEGER Unix seconds (UTC).
DROP TABLE IF EXISTS states;

CREATE TABLE blogs (
    id         TEXT PRIMARY KEY,
    owner_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    slug       TEXT NOT NULL UNIQUE,
    domain     TEXT UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_blogs_owner ON blogs (owner_id, created_at DESC, id DESC);

-- Exact browser origins allowed to call the public delivery API for a blog.
CREATE TABLE blog_origins (
    blog_id TEXT NOT NULL REFERENCES blogs(id) ON DELETE CASCADE,
    origin  TEXT NOT NULL,
    PRIMARY KEY (blog_id, origin)
);

CREATE TABLE posts (
    id               TEXT PRIMARY KEY,
    blog_id          TEXT NOT NULL REFERENCES blogs(id) ON DELETE CASCADE,
    author_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug             TEXT NOT NULL,
    title            TEXT NOT NULL,
    content_markdown TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'published')),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL,
    published_at     INTEGER,
    deleted_at       INTEGER,

    -- Deleted posts still occupy their slug so old URLs stay stable.
    UNIQUE (blog_id, slug)
);

-- Public listing: published, not deleted, newest first per blog.
CREATE INDEX idx_posts_blog_published
    ON posts (blog_id, status, published_at DESC, id DESC)
    WHERE deleted_at IS NULL;
