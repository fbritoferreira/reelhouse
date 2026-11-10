-- The provider's last refusal of a channel, in plain words; NULL once it plays.
ALTER TABLE channels ADD COLUMN refused TEXT;
