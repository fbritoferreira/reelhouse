-- When a refresh last listed the channel (Unix ms); one left out is kept a week (see refresh).
ALTER TABLE channels ADD COLUMN seen INTEGER NOT NULL DEFAULT 0;
