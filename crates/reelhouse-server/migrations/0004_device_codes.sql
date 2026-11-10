-- Sign-in codes shown on TVs and approved from a signed-in browser.
CREATE TABLE device_codes (
    code TEXT PRIMARY KEY,
    poll_hash TEXT UNIQUE NOT NULL,
    user_id INTEGER REFERENCES users ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
