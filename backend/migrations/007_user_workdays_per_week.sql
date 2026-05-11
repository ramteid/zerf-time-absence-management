ALTER TABLE users
    ADD COLUMN IF NOT EXISTS workdays_per_week SMALLINT NOT NULL DEFAULT 5
    CHECK (workdays_per_week >= 1 AND workdays_per_week <= 7);
