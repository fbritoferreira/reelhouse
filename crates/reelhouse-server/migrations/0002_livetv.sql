-- Times are Unix seconds.

CREATE TABLE tv_sources (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('m3u', 'xtream')),
    url TEXT NOT NULL,
    username TEXT,
    password TEXT,
    epg_url TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE channels (
    id INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES tv_sources ON DELETE CASCADE,
    number INTEGER,
    name TEXT NOT NULL,
    logo_url TEXT,
    stream_url TEXT NOT NULL,
    epg_channel_id TEXT,
    group_title TEXT,
    -- A refresh keeps the ids (and the rules pointing at them) of channels it finds again.
    UNIQUE (source_id, stream_url)
);

CREATE TABLE programmes (
    id INTEGER PRIMARY KEY,
    channel_id INTEGER NOT NULL REFERENCES channels ON DELETE CASCADE,
    start INTEGER NOT NULL,
    stop INTEGER NOT NULL,
    title TEXT NOT NULL,
    subtitle TEXT,
    description TEXT,
    series_key TEXT
);

CREATE INDEX programmes_channel_start ON programmes (channel_id, start);

CREATE TABLE recording_rules (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('once', 'series')),
    channel_id INTEGER NOT NULL REFERENCES channels ON DELETE CASCADE,
    -- Programmes are replaced on every guide refresh; a once rule gets its recording row when created.
    programme_id INTEGER REFERENCES programmes ON DELETE SET NULL,
    series_title TEXT,
    series_key TEXT,
    padding_before_s INTEGER NOT NULL DEFAULT 0,
    padding_after_s INTEGER NOT NULL DEFAULT 0,
    keep_last INTEGER
);

-- start and stop include the rule's padding: they are when ffmpeg runs.
CREATE TABLE recordings (
    id INTEGER PRIMARY KEY,
    rule_id INTEGER NOT NULL REFERENCES recording_rules ON DELETE CASCADE,
    channel_id INTEGER NOT NULL REFERENCES channels ON DELETE CASCADE,
    title TEXT NOT NULL,
    subtitle TEXT,
    start INTEGER NOT NULL,
    stop INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('scheduled', 'recording', 'done', 'failed')),
    file_path TEXT,
    item_id INTEGER REFERENCES items ON DELETE SET NULL
);

CREATE INDEX recordings_rule ON recordings (rule_id);
