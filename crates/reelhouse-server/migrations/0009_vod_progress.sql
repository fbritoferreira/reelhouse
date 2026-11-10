-- How far each person got in on-demand titles. Titles keep their ids across refreshes
-- (`seen` marks the refresh that last listed them), so progress survives one.
ALTER TABLE vod_items ADD COLUMN duration_s REAL;
ALTER TABLE vod_items ADD COLUMN seen INTEGER NOT NULL DEFAULT 0;
CREATE TABLE vod_progress (
    user_id INTEGER NOT NULL REFERENCES users ON DELETE CASCADE,
    vod_id INTEGER NOT NULL REFERENCES vod_items ON DELETE CASCADE,
    position_s REAL NOT NULL,
    played INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, vod_id)
);
