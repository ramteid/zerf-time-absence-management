-- Add tracks_time column to users.
-- When FALSE for an admin user, they are purely administrative: no time
-- tracking or absence management is available for them. All related
-- endpoints are blocked and navigation items are hidden in the frontend.
-- Default TRUE preserves existing behaviour for all users.
-- The CHECK constraint enforces that only admin users may have tracks_time=FALSE.
ALTER TABLE users ADD COLUMN IF NOT EXISTS tracks_time BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_admin_only_no_tracks_time;

ALTER TABLE users
    ADD CONSTRAINT users_admin_only_no_tracks_time
    CHECK (tracks_time = TRUE OR role = 'admin');
