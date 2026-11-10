CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users ON DELETE CASCADE,
    created_at INTEGER NOT NULL
);

CREATE TABLE libraries (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('movies', 'tv')),
    path TEXT NOT NULL
);

CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    library_id INTEGER NOT NULL REFERENCES libraries ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('movie', 'show', 'season', 'episode')),
    parent_id INTEGER REFERENCES items ON DELETE CASCADE,
    title TEXT NOT NULL,
    year INTEGER,
    season_no INTEGER,
    episode_no INTEGER,
    file_path TEXT UNIQUE,
    duration_s REAL,
    container TEXT,
    video_codec TEXT,
    audio_codec TEXT,
    height INTEGER,
    tmdb_id INTEGER,
    imdb_id TEXT,
    tvdb_id INTEGER,
    overview TEXT,
    poster_url TEXT,
    backdrop_url TEXT,
    added_at INTEGER NOT NULL
);

CREATE INDEX items_parent ON items (parent_id);
CREATE INDEX items_library ON items (library_id, kind);

CREATE TABLE progress (
    user_id INTEGER NOT NULL REFERENCES users ON DELETE CASCADE,
    item_id INTEGER NOT NULL REFERENCES items ON DELETE CASCADE,
    position_s REAL NOT NULL,
    played INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, item_id)
);
