-- Channel groups an admin has hidden from Live TV, per source. '' is the ungrouped.
CREATE TABLE hidden_groups (
    source_id INTEGER NOT NULL REFERENCES tv_sources ON DELETE CASCADE,
    group_title TEXT NOT NULL,
    PRIMARY KEY (source_id, group_title)
);
