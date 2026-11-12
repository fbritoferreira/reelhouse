-- Why a recording failed, in plain words; NULL unless it failed.
ALTER TABLE recordings ADD COLUMN failure TEXT;
