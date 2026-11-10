-- Background work (library scans, Live TV refreshes), run one at a time and retried on failure.
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    target_id INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    result TEXT,
    run_after_ms INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX jobs_due ON jobs (status, run_after_ms);
