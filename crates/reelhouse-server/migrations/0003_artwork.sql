-- When metadata was last looked up for an item (NULL = never), so failed lookups are not repeated every scan.
ALTER TABLE items ADD COLUMN metadata_at INTEGER;

-- Artwork downloaded once and served by the server itself.
CREATE TABLE images (
    item_id INTEGER NOT NULL REFERENCES items ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('poster', 'backdrop')),
    content_type TEXT NOT NULL,
    bytes BLOB NOT NULL,
    source_url TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (item_id, kind)
);
