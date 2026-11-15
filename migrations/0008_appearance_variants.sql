-- Named appearance variants: reusable styling snapshots per blog. A variant
-- stores a validated appearance document (same shape as blogs.
-- embed_appearance; see src/blog/appearance.rs); applying one is a normal
-- appearance write, so saving a variant never changes the live look itself.
--
-- One name per blog keeps apply flows unambiguous; deleting a blog cascades.

CREATE TABLE blog_appearance_variants (
    id         TEXT PRIMARY KEY,
    blog_id    TEXT NOT NULL REFERENCES blogs(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    appearance TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_appearance_variants_blog ON blog_appearance_variants (blog_id, created_at DESC, id DESC);

-- One name per blog, ignoring case, so `Cozy` and `cozy` cannot both exist.
CREATE UNIQUE INDEX idx_appearance_variants_name
    ON blog_appearance_variants (blog_id, name COLLATE NOCASE);
