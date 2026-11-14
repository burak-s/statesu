-- Saved embed appearance: one nullable, validated JSON document per blog
-- (schema versioned; see src/blog/appearance.rs). NULL means "use the
-- built-in appearance", so existing blogs and embeds keep their look.
--
-- Stored as typed style tokens, never CSS text: embed v2 turns them into
-- custom properties inside its shadow root.

ALTER TABLE blogs ADD COLUMN embed_appearance TEXT;
