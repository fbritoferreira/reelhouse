-- Films and series episodes a Live TV source offers on demand.
CREATE TABLE vod_items (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES tv_sources ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    year INTEGER,
    show_title TEXT,
    season_no INTEGER,
    episode_no INTEGER,
    poster_url TEXT,
    group_title TEXT,
    stream_url TEXT NOT NULL,
    UNIQUE (source_id, stream_url)
);
CREATE INDEX vod_by_group ON vod_items (kind, group_title);
CREATE INDEX vod_by_show ON vod_items (show_title, season_no, episode_no);
